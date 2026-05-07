use std::collections::{HashMap, HashSet, hash_map::Entry};

use crate::{
    ast::{IsResource, Mutable},
    diagnostics::DiagnosticReporter,
    resolved_ast::{LocalRegionId, VarId},
    typed_ast::{Expr, ExprKind, Function, Pattern, PatternKind, Place, PlaceKind},
    types::{FunctionType, GenericKind, Region, Type},
};
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
    line: usize,
    name: String,
    mutable: Mutable,
    function_level: usize,
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
    local_functions: usize,
    local_function: usize,
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
            local_functions: 0,
            local_function: 0,
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
    fn has_regions(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool
            | Type::Int
            | Type::String
            | Type::Unit
            | Type::Unknown
            | Type::Param(..)
            | Type::Function(..)
            | Type::Char => false,
            Type::List(ty) | Type::Box(ty) | Type::Option(ty) => self.has_regions(ty),
            Type::Imm(..) | Type::Mut(..) => true,
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
                let line = var_info.line;
                let msg = format!(
                    "'{}' cannot go out of scope",
                    var_info.name
                );
                self.err.report(msg, line);
            }
        }
        scope
    }
    fn init_var(&mut self, mutable: Mutable, var: VarId, line: usize, name: String, ty: Type) {
        self.scopes.last_mut().unwrap().push(var);
        self.vars.insert(
            var,
            VarInfo {
                line,
                ty,
                name,
                mutable,
                function_level: self.local_function,
            },
        );
        self.var_states.insert(var, VarState::Owned);
    }
    fn write_to_var(&mut self, var: VarId) {
        *self.var_states.get_mut(&var).unwrap() = VarState::Owned;
    }
    fn move_from_var(&mut self, var: VarId) {
        *self.var_states.get_mut(&var).unwrap() = VarState::Moved;
    }
    fn place_mutable(&self, place: &Place) -> Mutable {
        match &place.kind {
            PlaceKind::Var(var) => self.vars[&var.1].mutable,
            PlaceKind::Deref(value) => {
                let Ok((mutable, _, _)) = value.ty.clone().as_reference_type() else {
                    unreachable!("Should be a reference type at '{}'", place.line)
                };
                mutable
            }
        }
    }
    fn check_place_mutable(&mut self, place: &Place, kind: PlaceUse) {
        match (self.place_mutable(place), kind) {
            (Mutable::Immutable | Mutable::Mutable, PlaceUse::Read)
            | (Mutable::Mutable, PlaceUse::Write) => (),
            (Mutable::Immutable, PlaceUse::Write) => {
                self.err
                    .report("Cannot write to immutable place".to_string(), place.line);
            }
        }
    }
    fn check_place_use(&mut self, place: &Place, kind: PlaceUse) {
        check_place_use_inner(self, place, kind);
        fn check_place_use_inner(this: &mut ResourceCheck, place: &Place, kind: PlaceUse) {
            match &place.kind {
                PlaceKind::Deref(value) => {
                    let Ok((mutable, _, ty)) = value.ty.as_reference_type() else {
                        unreachable!("Should always be a reference")
                    };
                    match kind {
                        PlaceUse::Read => {
                            if mutable != Mutable::Mutable && this.is_resource(ty) {
                                this.err
                                    .report("Cannot move out from an imm".to_string(), place.line);
                            }
                        }
                        PlaceUse::Write => {
                            if mutable != Mutable::Mutable {
                                this.err
                                    .report("Cannot write with imm".to_string(), place.line);
                            }
                        }
                    }
                    this.check_expr(value, Some(kind));
                }
                PlaceKind::Var(var) => {
                    {
                        let state = &this.vars[&var.1];
                        if state.function_level != this.local_function
                            && (this.has_regions(&state.ty)
                                || (this.is_resource(&state.ty)
                                    && this.is_current_function_resource != IsResource::Resource))
                        {
                            this.err
                                .report(format!("Cannot capture variable '{}'", var.0), place.line);
                        }
                    }
                    match kind {
                        PlaceUse::Read => {
                            if let VarState::Moved =
                                this.var_states.get(&var.1).unwrap_or_else(|| {
                                    panic!(
                                        "Variable '{}' doesn't have state '{}'",
                                        var.0, place.line
                                    )
                                })
                            {
                                this.err.report(
                                    format!("Cannot use variable '{}' after move", var.0),
                                    place.line,
                                );
                            } else if this.is_resource(&place.ty) {
                                this.move_from_var(var.1);
                            }
                        }
                        PlaceUse::Write => {
                            this.write_to_var(var.1);
                        }
                    }
                }
            }
        }
    }
    fn check_pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::None | PatternKind::Bool(_) => (),
            PatternKind::Some(sub_pattern) | PatternKind::Deref(sub_pattern) => {
                self.check_pattern(sub_pattern);
            }
            PatternKind::Binding(mutable, var, ty) => {
                self.init_var(*mutable, var.1, pattern.line, var.0.clone(), (**ty).clone());
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr, ctxt: Option<PlaceUse>) {
        match &expr.kind {
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::None
            | ExprKind::Panic
            | ExprKind::Unit
            | ExprKind::String(_)
            | ExprKind::Int(_)
            | ExprKind::Function(..)
            | ExprKind::Builtin(..) => (),
            ExprKind::Some(value) => {
                self.check_expr(value, None);
            }
            ExprKind::Print(value) => {
                if let Some(value) = value {
                    self.check_expr(value, None);
                    if self.is_resource(&value.ty) {
                        self.err
                            .report(format!("Cannot print resource '{}'", value.ty), value.line);
                    }
                }
            }
            ExprKind::Call(callee, args) => {
                self.check_expr(callee, None);
                for arg in args {
                    self.check_expr(arg, None);
                }
            }
            ExprKind::List(values) => {
                for value in values {
                    self.check_expr(value, None);
                }
            }
            ExprKind::Load(place) => {
                self.check_place_use(place, ctxt.unwrap_or(PlaceUse::Read));
            }
            ExprKind::Binary(_, left, right) => {
                self.check_expr(left, None);
                self.check_expr(right, None);
            }
            ExprKind::Assign(place, value) => {
                self.check_expr(value, None);
                self.check_place_mutable(place, PlaceUse::Write);
                self.check_place_use(place, PlaceUse::Write);
            }
            ExprKind::Sequence(first, second) => {
                self.check_expr(first, None);
                if self.is_strict_resource(&first.ty) {
                    self.err.report(
                        format!("Cannot let '{}' out of scope", first.ty),
                        first.line,
                    );
                }
                self.check_expr(second, None);
            }
            ExprKind::Lambda(lambda) => {
                self.in_drop_scope(|this| {
                    let old_resource = std::mem::replace(
                        &mut this.is_current_function_resource,
                        lambda.is_resource,
                    );
                    let function = {
                        let old_function_count = this.local_functions;
                        this.local_functions += 1;
                        old_function_count
                    };
                    let old_function = std::mem::replace(&mut this.local_function, function);
                    for (name, var, ty) in lambda.params.iter() {
                        this.init_var(
                            Mutable::Immutable,
                            *var,
                            name.line,
                            name.content.clone(),
                            ty.clone(),
                        );
                    }
                    this.check_expr(&lambda.body, None);
                    this.is_current_function_resource = old_resource;
                    this.local_function = old_function;
                });
            }
            ExprKind::Let {
                pattern,
                binder,
                body,
            } => {
                self.in_drop_scope(|this| {
                    this.check_expr(binder, None);
                    this.check_pattern(pattern);
                    this.check_expr(body, None);
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
                        self.err.report(
                            format!("Cannot borrow '{}' as mut", var_name.content),
                            var_name.line,
                        );
                    }
                }
                self.init_var(
                    Mutable::Immutable,
                    *new_var,
                    var_name.line,
                    var_name.content.clone(),
                    new_ty.clone(),
                );
                self.check_expr(body, None);
                self.expired_regions.insert(*region);
                if self.ty_is_expired(&body.ty) {
                    self.err
                        .report(format!("Cannot let '{}' escape", body.ty), body.line);
                }
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
            } => {
                self.in_drop_scope(|this| {
                    this.check_expr(iterator, None);
                    this.check_pattern(pattern);
                    this.check_expr(body, None);
                });
            }
            ExprKind::Case(value, arms) => {
                self.check_expr(value, None);
                let mut combined_state = if arms.is_empty() {
                    self.var_states.clone()
                } else {
                    HashMap::new()
                };
                for arm in arms {
                    let old_state = self.var_states.clone();
                    self.in_drop_scope(|this| {
                        this.check_pattern(&arm.pattern);
                        this.check_expr(&arm.body, None);
                    });
                    let new_state = std::mem::replace(&mut self.var_states, old_state);
                    for (var, state) in new_state {
                        match combined_state.entry(var) {
                            Entry::Occupied(mut entry) => {
                                let Some(new_state) = unify_state(state, *entry.get()) else {
                                    let name = &self.vars[&var].name;
                                    self.err.report(
                                        format!("'{name}' should always be moved"),
                                        arm.body.line,
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
                    param.name.line,
                    param.name.content.clone(),
                    param.ty.clone(),
                );
            }
            this.check_expr(&function.body, None);
        });
        self.err.finish();
    }
}
