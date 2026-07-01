use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    ops::ControlFlow,
};

use crate::{
    Symbol,
    ast::{IsResource, Mutable},
    collect::CtxtRef,
    diagnostics::DiagnosticReporter,
    resolved_ast::{LocalRegionId, VarId},
    src_loc::SrcLoc,
    typed_ast::{
        Expr, ExprKind, Function, Param, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind,
    },
    types::{PointerType, Region, Type},
};

#[derive(Debug, Clone, Copy)]
enum CaptureError {
    NotAnUpvar,
    BorrowsLocal,
    DataFunction,
}
#[derive(Debug, Clone, Copy)]
enum PlaceUse {
    Read,
    Write,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum VarState {
    Owned,
    Moved,
}
struct VarInfo {
    ty: Type,
    loc: SrcLoc,
    name: Symbol,
    mutable: Mutable,
    function_level: usize,
    loop_count: usize,
}
fn unify_state(state1: VarState, state2: VarState) -> Option<VarState> {
    match (state1, state2) {
        (VarState::Moved, VarState::Moved) => Some(VarState::Moved),
        (VarState::Owned, VarState::Owned) => Some(VarState::Owned),
        (VarState::Owned, VarState::Moved) | (VarState::Moved, VarState::Owned) => None,
    }
}
pub struct ResourceCheck<'ctxt> {
    vars: HashMap<VarId, VarInfo>,
    var_states: HashMap<VarId, VarState>,
    err: DiagnosticReporter,
    is_current_function_resource: IsResource,
    expired_regions: HashSet<LocalRegionId>,
    borrowed: HashMap<VarId, (Mutable, Region)>,
    scopes: Vec<Vec<VarId>>,

    function_level: usize,
    capture_set: Option<HashMap<VarId, SrcLoc>>,
    loops: usize,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> ResourceCheck<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            ctxt,
            is_current_function_resource: IsResource::Data,
            vars: HashMap::new(),
            var_states: HashMap::new(),
            err: DiagnosticReporter::new(),
            scopes: Vec::new(),
            expired_regions: HashSet::new(),
            function_level: 0,
            capture_set: None,
            loops: 0,
            borrowed: HashMap::new(),
        }
    }
    fn is_strict_resource(&self, _: &Type) -> bool {
        false
    }
    fn ty_is_expired(&self, ty: &Type) -> bool {
        ty.visit(&mut Type::no_op_visit, &mut |region| {
            if let Region::Local(_, ref local) = region
                && self.expired_regions.contains(local)
            {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_break()
    }
    fn in_drop_scope(&mut self, f: impl FnOnce(&mut Self)) -> Vec<VarId> {
        self.scopes.push(Default::default());
        f(self);
        let Some(scope) = self.scopes.pop() else {
            return Vec::new();
        };
        for &var in &scope {
            let var_info = &self.vars[&var];
            let state = self.var_states[&var];
            if state == VarState::Owned && self.is_strict_resource(&var_info.ty) {
                let loc = var_info.loc;
                let msg = format!("'{}' cannot go out of scope", var_info.name);
                self.err.add_diagnostic(msg, loc);
            }
            self.borrowed.remove(&var);
        }
        scope
    }
    fn init_var(
        &mut self,
        mutable: Mutable,
        var: VarId,
        loc: SrcLoc,
        name: Symbol,
        ty: Type,
        _is_param: bool,
    ) {
        self.scopes.last_mut().unwrap().push(var);
        self.vars.insert(
            var,
            VarInfo {
                loc,
                ty,
                name,
                mutable,
                function_level: self.function_level,
                loop_count: self.loops,
            },
        );
        self.var_states.insert(var, VarState::Owned);
    }
    fn write_to_var(&mut self, var: VarId, loc: SrcLoc) {
        let info = &self.vars[&var];
        if self.borrowed.contains_key(&var) {
            self.err.add_diagnostic(
                format!("Cant assign to '{}' while borrowed", info.name),
                loc,
            );
            return;
        }
        let is_resource = self.is_strict_resource(&info.ty);
        let state = self.var_states.get_mut(&var).unwrap();
        if is_resource && *state != VarState::Moved {
            self.err.add_diagnostic(
                format!("Cant assign to '{}' while not moved", info.name),
                loc,
            );
        }
        *state = VarState::Owned;
    }
    fn move_from_var(&mut self, var: VarId, loc: SrcLoc) {
        let info = &self.vars[&var];
        let is_resource = info.ty.is_resource(self.ctxt);
        let state = self.var_states.get_mut(&var).unwrap();
        match state {
            VarState::Owned => {
                if self.borrowed.contains_key(&var) {
                    self.err.add_diagnostic(
                        format!("Cannot move from '{}' while borrowed", info.name),
                        loc,
                    );
                    return;
                }
                if is_resource {
                    *state = VarState::Moved;
                    let info = &self.vars[&var];
                    if self.loops > 0 && info.loop_count != self.loops {
                        self.err.add_diagnostic(
                            format!("Cannot move from '{}' in a loop", info.name),
                            loc,
                        );
                    }
                }
            }
            VarState::Moved => {
                self.err.add_diagnostic(
                    format!("Cannot use '{}' after move", self.vars[&var].name),
                    loc,
                );
            }
        }
    }
    fn place_mutable(&self, place: &Place) -> Mutable {
        match &place.kind {
            PlaceKind::Var(var) | PlaceKind::Upvar(var) => self.vars[&var.1].mutable,
            PlaceKind::Deref(_) => {
                let mut place = place;
                while let PlaceKind::Deref(value) = &place.kind {
                    let mutable = match value.ty.clone().as_pointer_type() {
                        Ok((PointerType::Raw, _)) => Mutable::Mutable,
                        Ok((PointerType::Reference(_, mutable), _)) => mutable,
                        _ => unreachable!(),
                    };
                    if mutable == Mutable::Immutable {
                        return Mutable::Immutable;
                    }
                    let ExprKind::Load(new_place) = &value.kind else {
                        break;
                    };
                    place = new_place;
                }
                Mutable::Mutable
            }
        }
    }
    fn check_place_mutable(&mut self, place: &Place) {
        match self.place_mutable(place) {
            Mutable::Immutable => self
                .err
                .add_diagnostic("Cannot write to immutable place".to_string(), place.loc),
            Mutable::Mutable => (),
        }
    }
    fn check_pattern(&mut self, pattern: &Pattern, in_ref: bool) {
        match &pattern.kind {
            PatternKind::Bool(_) | PatternKind::Int(_) | PatternKind::Err => (),
            PatternKind::Record(fields) => {
                for field in fields {
                    self.check_pattern(&field.pattern, in_ref);
                }
            }
            PatternKind::Ref(sub_pattern) => {
                self.check_pattern(sub_pattern, true);
            }
            PatternKind::Case(.., inner) => {
                if let Some(inner) = inner {
                    self.check_pattern(inner, in_ref);
                }
            }
            PatternKind::Binding(borrow, mutable, var, ty) => {
                if in_ref && ty.is_resource(self.ctxt) && borrow.is_none() {
                    self.err
                        .add_diagnostic("Cannot move out of reference".to_string(), pattern.loc)
                }
                self.init_var(*mutable, var.1, pattern.loc, var.0, (**ty).clone(), false);
            }
        }
    }
    fn regions_in(&self, ty: &Type) -> HashSet<Region> {
        let mut regions = HashSet::new();
        let ControlFlow::Continue(()) = ty.visit(
            &mut Type::no_op_visit::<std::convert::Infallible>,
            &mut |region| {
                regions.insert(region);
                ControlFlow::Continue(())
            },
        );
        regions
    }
    fn outlives_generic_regions(&self, ty: &Type) -> bool {
        ty.visit(&mut Type::no_op_visit, &mut |region| {
            if matches!(region, Region::Unknown | Region::Static | Region::Param(..)) {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .is_continue()
    }
    fn var_of(&self, place: &Place) -> Option<VarId> {
        match place.kind {
            PlaceKind::Var(ref var) | PlaceKind::Upvar(ref var) => Some(var.1),
            PlaceKind::Deref(ref value) => {
                if let ExprKind::Load(ref place) = value.kind {
                    self.var_of(place)
                } else {
                    None
                }
            }
        }
    }
    fn capture_valid(&self, var: VarId) -> Result<(), CaptureError> {
        let info = &self.vars[&var];
        if info.function_level == self.function_level {
            return Err(CaptureError::NotAnUpvar);
        }
        //Function is not a resource, can't capture anything
        if self.is_current_function_resource == IsResource::Data {
            return Err(CaptureError::DataFunction);
        }
        if !self.outlives_generic_regions(&info.ty) {
            return Err(CaptureError::BorrowsLocal);
        }
        Ok(())
    }
    fn capture_if_upvar(&mut self, var: VarId, loc: SrcLoc) {
        let cause = match self.capture_valid(var) {
            Ok(()) => {
                let capture_set = self.capture_set.as_mut().expect("Should have capture set");
                capture_set.insert(var, loc);
                return;
            }
            Err(CaptureError::NotAnUpvar) => return,
            Err(CaptureError::DataFunction) => "because 'data' functions cannot capture",
            Err(CaptureError::BorrowsLocal) => "because borrowed content cannot be captured",
        };
        self.err.add_diagnostic(
            format!("Cannot capture '{}' {}", self.vars[&var].name, cause),
            loc,
        );
    }
    fn check_place_use(&mut self, place: &Place, place_use: PlaceUse) {
        match &place.kind {
            PlaceKind::Var(var) | PlaceKind::Upvar(var) => {
                let var = var.1;
                self.capture_if_upvar(var, place.loc);
                match place_use {
                    PlaceUse::Write => {
                        self.write_to_var(var, place.loc);
                    }
                    PlaceUse::Read => {
                        self.move_from_var(var, place.loc);
                    }
                }
            }
            PlaceKind::Deref(expr) => match &expr.kind {
                ExprKind::Load(place) => {
                    let Ok((_, ref ty)) = place.ty.clone().as_pointer_type() else {
                        unreachable!()
                    };
                    let Some(var) = self.var_of(place) else {
                        return self.check_place_use(place, place_use);
                    };
                    self.capture_if_upvar(var, place.loc);
                    match place_use {
                        PlaceUse::Read => {
                            if ty.is_resource(self.ctxt) {
                                self.err.add_diagnostic(
                                    "Cannot move out of reference".to_string(),
                                    place.loc,
                                )
                            }
                        }
                        PlaceUse::Write => {
                            if self.is_strict_resource(ty) {
                                self.err.add_diagnostic(
                                    "Cannot re-assign to reference".to_string(),
                                    place.loc,
                                )
                            }
                        }
                    }
                }
                _ => self.check_expr(expr),
            },
        }
    }
    fn check_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                self.check_expr(expr);
                if self.is_strict_resource(&expr.ty) {
                    self.err
                        .add_diagnostic(format!("Cannot let '{}' out of scope", expr.ty), expr.loc);
                }
            }
            StmtKind::Let(let_binding) => {
                self.check_pattern(&let_binding.pattern, false);
                self.check_expr(&let_binding.value);
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Block(body, region) => {
                self.in_drop_scope(|this| {
                    for stmt in &body.stmts {
                        this.check_stmt(stmt);
                    }
                    this.check_expr(&body.expr);
                    if let Some(region) = region {
                        this.expired_regions.insert(*region);
                        if this.ty_is_expired(&body.expr.ty) {
                            this.err.add_diagnostic(
                                format!("Cannot let '{}' escape", body.expr.ty),
                                expr.loc,
                            );
                        }
                        this.borrowed.retain(|_, (_, region)| {
                            let Region::Local(_, local) = region else {
                                return true;
                            };
                            !this.expired_regions.contains(local)
                        });
                    }
                });
            }
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::Panic
            | ExprKind::Unit
            | ExprKind::String(_)
            | ExprKind::Int(_)
            | ExprKind::Const(..) => {}
            ExprKind::Function(..) => {}
            ExprKind::VariantInit(.., value) => {
                self.check_expr(value);
            }
            ExprKind::Print(value) => {
                if let Some(value) = value {
                    self.check_expr(value);
                    if value.ty.is_resource(self.ctxt) {
                        self.err.add_diagnostic(
                            format!("Cannot print resource '{}'", value.ty),
                            value.loc,
                        );
                    }
                }
            }
            ExprKind::Call(callee, args) => {
                self.check_expr(callee);
                for arg in args {
                    self.check_expr(arg);
                }
            }
            ExprKind::BuiltinCall(.., args) => {
                for arg in args {
                    self.check_expr(arg);
                }
            }
            ExprKind::List(values) => {
                for value in values {
                    self.check_expr(value);
                }
            }
            ExprKind::Record(fields) => {
                for field in fields {
                    self.check_expr(&field.value);
                }
            }
            ExprKind::Load(place) => self.check_place_use(place, PlaceUse::Read),
            ExprKind::Binary(_, left, right) => {
                self.check_expr(left);
                self.check_expr(right);
            }
            ExprKind::Assign(place, value) => {
                self.check_expr(value);
                self.check_place_mutable(place);
                self.check_place_use(place, PlaceUse::Write);
            }
            ExprKind::Lambda(lambda) => {
                self.in_drop_scope(|this| {
                    let capture_info = this.capture_set.replace(Default::default());
                    let old_resource = std::mem::replace(
                        &mut this.is_current_function_resource,
                        lambda.is_resource,
                    );
                    this.function_level += 1;
                    for Param { name, var, ty } in lambda.params.iter() {
                        this.init_var(
                            Mutable::Immutable,
                            *var,
                            name.loc,
                            name.symbol,
                            ty.clone(),
                            true,
                        );
                    }
                    this.check_expr(&lambda.body);
                    if let Some(captures) = this.capture_set.as_ref() {
                        let mut errors = Vec::new();
                        for (&var, &loc) in captures {
                            let var_info = &this.vars[&var];
                            if this.regions_in(&var_info.ty).iter().any(|region| {
                                *region != Region::Static || *region != Region::Unknown
                            }) {
                                errors.push((var_info.name, loc));
                            }
                        }
                        errors.sort_by_key(|(_, loc)| loc.line);
                        for (name, loc) in errors {
                            this.err.add_diagnostic(
                                format!("Cannot capture '{}' that contains borrows", name),
                                loc,
                            );
                        }
                    }
                    this.is_current_function_resource = old_resource;
                    this.function_level -= 1;
                    this.capture_set = capture_info;
                });
            }
            &ExprKind::Borrow {
                mutable,
                ref place,
                region,
            } => {
                let old_var_mutable = self.place_mutable(place);
                match (old_var_mutable, mutable) {
                    (Mutable::Mutable, _) | (Mutable::Immutable, Mutable::Immutable) => (),
                    (Mutable::Immutable, Mutable::Mutable) => {
                        self.err
                            .add_diagnostic("Cannot borrow  as mut".to_string(), place.loc);
                    }
                }

                let var = match &place.kind {
                    PlaceKind::Upvar(var) => var,
                    PlaceKind::Var(var) => var,
                    PlaceKind::Deref(_) => {
                        self.err
                            .add_diagnostic("Cannot borrow this place".to_string(), place.loc);
                        return;
                    }
                };
                if self
                    .var_states
                    .get(&var.1)
                    .is_some_and(|state| *state == VarState::Moved)
                {
                    self.err.add_diagnostic(
                        format!("Cannot borrow '{}' while moved", var.0),
                        place.loc,
                    );
                    return;
                }
                // TODO : Allow borrowing from place with longer region
                if !matches!(region, Region::Local(..)) {
                    self.err.add_diagnostic(
                        format!("Cannot borrow '{}' with region '{}'", var.0, region),
                        place.loc,
                    );
                }
                match self.borrowed.entry(var.1) {
                    Entry::Vacant(entry) => {
                        entry.insert_entry((mutable, region));
                    }
                    Entry::Occupied(entry) => match (entry.get().0, mutable) {
                        (Mutable::Immutable, Mutable::Immutable) => (),
                        (_, Mutable::Mutable) => {
                            self.err
                                .add_diagnostic("Cannot borrow as mut".to_string(), place.loc);
                        }
                        (Mutable::Mutable, Mutable::Immutable) => {
                            self.err
                                .add_diagnostic("Cannot borrow as imm".to_string(), place.loc);
                        }
                    },
                }
            }
            ExprKind::For {
                pattern,
                iterator,
                iterator_type: _,
                body,
            } => {
                self.check_expr(iterator);
                let new_loop = self.loops + 1;
                let old_loop = std::mem::replace(&mut self.loops, new_loop);

                self.in_drop_scope(|this| {
                    this.check_pattern(pattern, false);
                    this.check_expr(body);
                });
                self.loops = old_loop;
            }
            ExprKind::Case(value, arms) => {
                self.check_expr(value);
                let mut combined_state = if arms.is_empty() {
                    self.var_states.clone()
                } else {
                    HashMap::new()
                };
                for arm in arms {
                    let old_state = self.var_states.clone();
                    self.in_drop_scope(|this| {
                        this.check_pattern(&arm.pattern, false);
                        this.check_expr(&arm.body);
                    });
                    let new_state = std::mem::replace(&mut self.var_states, old_state);
                    for (var, state) in new_state {
                        match combined_state.entry(var) {
                            Entry::Occupied(mut entry) => {
                                let new_state =
                                    unify_state(state, *entry.get()).unwrap_or(VarState::Moved);
                                entry.insert(new_state);
                            }
                            Entry::Vacant(entry) => {
                                entry.insert(state);
                            }
                        }
                    }
                }
                self.var_states = combined_state;
            }
        }
    }
    pub fn check_function(mut self, function: &Function) -> bool {
        self.in_drop_scope(|this| {
            for param in function.params.iter() {
                this.init_var(
                    Mutable::Immutable,
                    param.var,
                    param.name.loc,
                    param.name.symbol,
                    param.ty.clone(),
                    true,
                );
            }
            if let Some(ref body) = function.body {
                this.check_expr(body);
            }
        });
        self.err.report_all()
    }
}
