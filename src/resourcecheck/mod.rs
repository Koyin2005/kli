use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
    rc::Rc,
};

use crate::{
    ast::{IsResource, Mutable},
    diagnostics::DiagnosticReporter,
    resolved_ast::{LocalRegionId, VarId},
    src_loc::SrcLoc,
    typed_ast::{Expr, ExprKind, Function, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind},
    types::{FunctionType, GenericKind, Region, Type},
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
    name: Rc<str>,
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
impl Default for ResourceCheck {
    fn default() -> Self {
        Self::new()
    }
}
pub struct ResourceCheck {
    vars: HashMap<VarId, VarInfo>,
    var_states: HashMap<VarId, VarState>,
    err: DiagnosticReporter,
    is_current_function_resource: IsResource,
    expired_regions: HashSet<LocalRegionId>,
    scopes: Vec<Vec<VarId>>,
    region_params: HashSet<usize>,
    function_level: usize,
    capture_set: Option<HashMap<VarId, SrcLoc>>,
    loops: usize,
}
impl ResourceCheck {
    pub fn new() -> Self {
        Self {
            is_current_function_resource: IsResource::Data,
            region_params: HashSet::new(),
            vars: HashMap::new(),
            var_states: HashMap::new(),
            err: DiagnosticReporter::new(),
            scopes: Vec::new(),
            expired_regions: HashSet::new(),
            function_level: 0,
            capture_set: None,
            loops: 0,
        }
    }
    fn is_strict_resource(&self, ty: &Type) -> bool {
        !matches!(ty, Type::Mut(..)) && self.is_resource(ty)
    }
    fn is_resource(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool
            | Type::Unit
            | Type::Unknown
            | Type::Int
            | Type::Imm(..)
            | Type::Char
            | Type::Function(FunctionType {
                resource: IsResource::Data,
                ..
            }) => false,
            Type::Option(ty) => self.is_resource(ty),
            Type::Mut(..)
            | Type::Function(FunctionType {
                resource: IsResource::Resource,
                ..
            })
            | Type::String
            | Type::Box(_)
            | Type::Param(..)
            | Type::List(_) => true,
            Type::Infer(_) => unreachable!("All infers should be removed"),
        }
    }
    fn ty_is_expired(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool
            | Type::Int
            | Type::String
            | Type::Unit
            | Type::Unknown
            | Type::Param(..)
            | Type::Function(..)
            | Type::Char => false,
            Type::List(ty) | Type::Box(ty) | Type::Option(ty) => self.ty_is_expired(ty),
            Type::Imm(region, ty) | Type::Mut(region, ty) => {
                if let Region::Local(_, local) = region
                    && self.expired_regions.contains(local)
                {
                    return true;
                }
                self.ty_is_expired(ty)
            }
            Type::Infer(_) => unreachable!("All infers should be removed"),
        }
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
                let loc = var_info.loc.clone();
                let msg = format!("'{}' cannot go out of scope", var_info.name);
                self.err.add_diagnostic(msg, loc);
            }
        }
        scope
    }
    fn init_var(
        &mut self,
        mutable: Mutable,
        var: VarId,
        loc: SrcLoc,
        name: Rc<str>,
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
        let is_resource = self.is_resource(&info.ty);
        let state = self.var_states.get_mut(&var).unwrap();
        match state {
            VarState::Owned => {
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
            PlaceKind::Var(var) => self.vars[&var.1].mutable,
            PlaceKind::Deref(_) => {
                let mut place = place;
                while let PlaceKind::Deref(value) = &place.kind {
                    let Ok((mutable, _, _)) = value.ty.as_reference_type() else {
                        unreachable!()
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
            Mutable::Immutable => self.err.add_diagnostic(
                "Cannot write to immutable place".to_string(),
                place.loc.clone(),
            ),
            Mutable::Mutable => (),
        }
    }
    fn check_pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::None | PatternKind::Bool(_) => (),
            PatternKind::Some(sub_pattern) | PatternKind::Deref(sub_pattern) => {
                self.check_pattern(sub_pattern);
            }
            PatternKind::Binding(mutable, var, ty) => {
                self.init_var(
                    *mutable,
                    var.1,
                    pattern.loc.clone(),
                    var.0.clone(),
                    (**ty).clone(),
                    false,
                );
            }
        }
    }
    fn regions_in(&self, ty: &Type) -> HashSet<Region> {
        match ty {
            Type::Bool
            | Type::Char
            | Type::Int
            | Type::String
            | Type::Unit
            | Type::Param(..)
            | Type::Unknown => HashSet::new(),
            Type::Infer(_) => unreachable!("Cannot infer here"),
            Type::Box(ty) | Type::List(ty) | Type::Option(ty) => self.regions_in(ty),
            Type::Function(function) => {
                function.params.iter().fold(HashSet::new(), |old, param| {
                    let mut old = old;
                    old.extend(self.regions_in(param));
                    old
                })
            }
            Type::Imm(region, ty) | Type::Mut(region, ty) => {
                let mut regions = HashSet::new();
                regions.insert(region.clone());
                regions.extend(self.regions_in(ty));
                regions
            }
        }
    }
    fn outlives_generic_regions(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool
            | Type::Char
            | Type::Int
            | Type::String
            | Type::Unit
            | Type::Param(..)
            | Type::Unknown => true,
            Type::Infer(_) => unreachable!("Cannot infer here"),
            Type::Box(ty) | Type::List(ty) | Type::Option(ty) => self.outlives_generic_regions(ty),
            Type::Function(function) => {
                function
                    .params
                    .iter()
                    .all(|param| self.outlives_generic_regions(param))
                    && self.outlives_generic_regions(&function.return_type)
            }
            Type::Imm(region, ty) | Type::Mut(region, ty) => {
                if !matches!(region, Region::Unknown | Region::Static | Region::Param(..)) {
                    return false;
                }
                self.outlives_generic_regions(ty)
            }
        }
    }
    fn var_of(&self, place: &Place) -> Option<VarId> {
        match place.kind {
            PlaceKind::Var(ref var) => Some(var.1),
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
            PlaceKind::Var(var) => {
                let var = var.1;
                self.capture_if_upvar(var, place.loc.clone());
                match place_use {
                    PlaceUse::Write => {
                        self.write_to_var(var, place.loc.clone());
                    }
                    PlaceUse::Read => {
                        self.move_from_var(var, place.loc.clone());
                    }
                }
            }
            PlaceKind::Deref(expr) => match &expr.kind {
                ExprKind::Load(place) => {
                    let Ok((_, _, ty)) = place.ty.as_reference_type() else {
                        unreachable!()
                    };
                    let Some(var) = self.var_of(place) else {
                        return self.check_place_use(place, place_use);
                    };
                    self.capture_if_upvar(var, place.loc.clone());
                    if !self.is_resource(ty) {
                        return;
                    }
                    match place_use {
                        PlaceUse::Read => self.err.add_diagnostic(
                            "Cannot move out of reference".to_string(),
                            place.loc.clone(),
                        ),
                        PlaceUse::Write => self.err.add_diagnostic(
                            "Cannot re-assign reference".to_string(),
                            place.loc.clone(),
                        ),
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
                    self.err.add_diagnostic(
                        format!("Cannot let '{}' out of scope", expr.ty),
                        expr.loc.clone(),
                    );
                }
            }
            StmtKind::Let(let_binding) => {
                self.check_pattern(&let_binding.pattern);
                self.check_expr(&let_binding.value);
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Block(body) => {
                for stmt in &body.stmts {
                    self.check_stmt(stmt);
                }
                self.check_expr(&body.expr);
            }
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::None
            | ExprKind::Panic
            | ExprKind::Unit
            | ExprKind::String(_)
            | ExprKind::Int(_)
            | ExprKind::Builtin(..) => (),
            ExprKind::Function(..) => {}
            ExprKind::Some(value) => {
                self.check_expr(value);
            }
            ExprKind::Print(value) => {
                if let Some(value) = value {
                    self.check_expr(value);
                    if self.is_resource(&value.ty) {
                        self.err.add_diagnostic(
                            format!("Cannot print resource '{}'", value.ty),
                            value.loc.clone(),
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
            ExprKind::List(values) => {
                for value in values {
                    self.check_expr(value);
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
                    for (name, var, ty) in lambda.params.iter() {
                        this.init_var(
                            Mutable::Immutable,
                            *var,
                            name.loc.clone(),
                            name.content.clone(),
                            ty.clone(),
                            true,
                        );
                    }
                    this.check_expr(&lambda.body);
                    if let Some(captures) = this.capture_set.as_ref() {
                        let mut errors = Vec::new();
                        for (var, loc) in captures {
                            let var_info = &this.vars[var];
                            if this.regions_in(&var_info.ty).iter().any(|region| {
                                *region != Region::Static || *region != Region::Unknown
                            }) {
                                errors.push((var_info.name.clone(), loc));
                            }
                        }
                        errors.sort_by_key(|(_, loc)| loc.line);
                        for (name, loc) in errors {
                            this.err.add_diagnostic(
                                format!("Cannot capture '{}' that contains borrows", name),
                                loc.clone(),
                            );
                        }
                    }
                    this.is_current_function_resource = old_resource;
                    this.function_level -= 1;
                    this.capture_set = capture_info;
                });
            }
            ExprKind::Borrow {
                var_name,
                new_var,
                new_ty,
                region,
                body,
                old_var,
                mutable,
                ..
            } => {
                let old_var_mutable = self.vars[old_var].mutable;
                match (old_var_mutable, mutable) {
                    (Mutable::Mutable, Mutable::Immutable)
                    | (Mutable::Mutable, Mutable::Mutable)
                    | (Mutable::Immutable, Mutable::Immutable) => (),
                    (Mutable::Immutable, Mutable::Mutable) => {
                        self.err.add_diagnostic(
                            format!("Cannot borrow '{}' as mut", var_name.content),
                            var_name.loc.clone(),
                        );
                    }
                }
                self.init_var(
                    Mutable::Immutable,
                    *new_var,
                    var_name.loc.clone(),
                    var_name.content.clone(),
                    new_ty.clone(),
                    false,
                );
                self.check_expr(body);
                self.expired_regions.insert(*region);
                if self.ty_is_expired(&body.ty) {
                    self.err.add_diagnostic(
                        format!("Cannot let '{}' escape", body.ty),
                        body.loc.clone(),
                    );
                }
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
            } => {
                self.check_expr(iterator);
                let new_loop = self.loops + 1;
                let old_loop = std::mem::replace(&mut self.loops, new_loop);

                self.in_drop_scope(|this| {
                    this.check_pattern(pattern);
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
                        this.check_pattern(&arm.pattern);
                        this.check_expr(&arm.body);
                    });
                    let new_state = std::mem::replace(&mut self.var_states, old_state);
                    for (var, state) in new_state {
                        match combined_state.entry(var) {
                            Entry::Occupied(mut entry) => {
                                let Some(new_state) = unify_state(state, *entry.get()) else {
                                    let name = &self.vars[&var].name;
                                    self.err.add_diagnostic(
                                        format!("'{name}' should always be moved"),
                                        arm.body.loc.clone(),
                                    );
                                    continue;
                                };
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
    pub fn check_function(mut self, function: &Function) {
        self.region_params.extend(
            function
                .generics
                .iter()
                .enumerate()
                .filter_map(|(i, param)| match param.kind {
                    GenericKind::Region => Some(i),
                    _ => None,
                }),
        );
        self.in_drop_scope(|this| {
            for param in function.params.iter() {
                this.init_var(
                    Mutable::Immutable,
                    param.var,
                    param.name.loc.clone(),
                    param.name.content.clone(),
                    param.ty.clone(),
                    true,
                );
            }
            this.check_expr(&function.body);
        });
        self.err.report_all();
    }
}
