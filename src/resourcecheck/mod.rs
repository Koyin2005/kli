use std::collections::{HashMap, HashSet};

use crate::{
    ast::Mutable, diagnostics::DiagnosticReporter, resolved_ast::{LocalRegionId, VarId}, typed_ast::{Expr, ExprKind, Function, GenericParam, Pattern, PatternKind, Place, PlaceKind}, types::{GenericKind, Region, Type}
};
#[derive(Debug)]
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
    state: VarState,
    ty: Type,
    name: String,
    function_level : usize
}
pub struct ResourceCheck {
    vars: HashMap<VarId, VarInfo>,
    err: DiagnosticReporter,
    expired_regions : HashSet<LocalRegionId>,
    scopes: Vec<Vec<VarId>>,
    region_params : HashSet<usize>,
    local_function : usize,
}
impl ResourceCheck {
    pub fn new() -> Self {
        Self {
            region_params : HashSet::new(),
            vars: HashMap::new(),
            err: DiagnosticReporter::new(),
            scopes: Vec::new(),
            expired_regions : HashSet::new(),
            local_function : 0
        }
    }
    fn is_strict_resource(&self, ty: &Type) -> bool{
        !matches!(ty,Type::Mut(..)) && self.is_resource(ty)
    }
    fn is_resource(&self, ty: &Type) -> bool {
        match ty {
            Type::Bool | Type::Unit | Type::Unknown | Type::Int | Type::Imm(..) => false,
            Type::Option(ty) => self.is_resource(ty),
            Type::Mut(..)
            | Type::Function(_)
            | Type::String
            | Type::Box(_)
            | Type::Param(..)
            | Type::List(_) => true,
            Type::Infer(_) => unreachable!("All infers should be removed"),
        }
    }
    fn ty_is_expired(&self, ty: &Type) -> bool{
        match ty {
            Type::Bool | Type::Int | Type::String | Type::Unit | Type::Unknown | Type::Param(..) | Type::Function(..) => false,
            Type::List(ty) | Type::Box(ty) | Type::Option(ty)  => self.ty_is_expired(ty),
            Type::Imm(region,ty) | Type::Mut(region,ty) => {
                if let Region::Local(_,local) = region
                    && self.expired_regions.contains(local){
                        return true;
                }
                self.ty_is_expired(ty)
            }
            Type::Infer(_) => unreachable!("All infers should be removed")
        }
    }
    fn has_regions(&self, ty: &Type) -> bool{
        match ty {
            Type::Bool | Type::Int | Type::String | Type::Unit | Type::Unknown | Type::Param(..) | Type::Function(..) => false,
            Type::List(ty) | Type::Box(ty) | Type::Option(ty)  => self.has_regions(ty),
            Type::Imm(..) | Type::Mut(..) => true,
            Type::Infer(_) => unreachable!("All infers should be removed")
        }
    }
    fn in_drop_scope(&mut self, line: usize, f: impl FnOnce(&mut Self)) {
        self.scopes.push(Default::default());
        f(self);
        let Some(scope) = self.scopes.pop() else {
            return;
        };
        for var in scope {
            let var_info = &self.vars[&var];
            if var_info.state == VarState::Owned && self.is_strict_resource(&var_info.ty) {
                let msg = format!(
                    "'{}' cannot go out of scope",
                    var_info.name
                );
                self.err.report(msg, line);
            }
        }
    }
    fn init_var(&mut self, var: VarId, name: String, ty: Type) {
        self.scopes.last_mut().unwrap().push(var);
        self.vars.insert(
            var,
            VarInfo {
                state: VarState::Owned,
                ty: ty,
                name,
                function_level: self.local_function
            },
        );
    }
    fn write_to_var(&mut self, var: VarId) {
        self.vars.get_mut(&var).unwrap().state = VarState::Owned;
    }
    fn move_from_var(&mut self, var: VarId) {
        let info = self.vars.get_mut(&var).unwrap();
        info.state = VarState::Moved;
    }
    fn check_place_use(&mut self, place: &Place, kind: PlaceUse) {
        match &place.kind {
            PlaceKind::Deref(value) => {
                let Ok((mutable,_,ty)) = value.ty.clone().as_reference_type() else {
                    unreachable!("Should always be a reference")
                };
                match kind{
                    PlaceUse::Read => {
                        if mutable != Mutable::Mutable && self.is_resource(&ty){
                            self.err.report("Cannot move out from an imm reference".to_string(),place.line);
                        }
                    },
                    PlaceUse::Write => {
                        if mutable != Mutable::Mutable{
                            self.err.report("Cannot write with imm reference".to_string(),place.line);
                        }
                    }
                }
                self.check_expr(value,Some(kind));
            },
            PlaceKind::Var(var) => {
                {
                    let state = &self.vars[&var.1];
                    if state.function_level != self.local_function && self.has_regions(&state.ty){
                        self.err.report(format!("Cannot capture variable '{}'",var.0), place.line);
                    }
                }
                match kind {
                    PlaceUse::Read => {
                        if let VarState::Moved = self.vars[&var.1].state {
                            self.err.report(
                                format!("Cannot use variable '{}' after move", var.0),
                                place.line,
                            );
                        } else if self.is_resource(&place.ty) {
                            self.move_from_var(var.1);
                        }
                    }
                    PlaceUse::Write => {
                        self.write_to_var(var.1);
                    }
                }
            },
        }
    }
    fn check_pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            PatternKind::None => (),
            PatternKind::Some(sub_pattern) | PatternKind::Deref(sub_pattern) => {
                self.check_pattern(sub_pattern);
            }
            PatternKind::Binding(mutable, var, ty) => {
                self.init_var(var.1, var.0.clone(), (**ty).clone());
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr, ctxt : Option<PlaceUse>) {
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
                self.check_expr(value,None);
            }
            ExprKind::Print(value) => {
                if let Some(value) = value {
                    self.check_expr(value,None);
                    if self.is_resource(&value.ty){
                        self.err.report(format!("Cannot print resource '{}'",value.ty), value.line);
                    }
                }
            }
            ExprKind::Call(callee, args) => {
                self.check_expr(callee,None);
                for arg in args {
                    self.check_expr(arg,None);
                }
            }
            ExprKind::List(values) => {
                for value in values {
                    self.check_expr(value,None);
                }
            }
            ExprKind::Load(place) => {
                self.check_place_use(place, ctxt.unwrap_or(PlaceUse::Read));
            }
            ExprKind::Binary(_, left, right) => {
                self.check_expr(left,None);
                self.check_expr(right,None);
            }
            ExprKind::Assign(place, value) => {
                self.check_expr(value,None);
                self.check_place_use(place, PlaceUse::Write);
            }
            ExprKind::Sequence(first, second) => {
                self.check_expr(first,None);
                if self.is_strict_resource(&first.ty) {
                    self.err.report(
                        format!("Cannot let '{}' out of scope", first.ty),
                        first.line,
                    );
                }
                self.check_expr(second,None);
            }
            ExprKind::Lambda(lambda) => {
                self.in_drop_scope(lambda.body.line, |this|{
                    this.local_function += 1;
                    for (name,var,ty) in lambda.params.iter(){
                        this.init_var(*var, name.content.clone(), ty.clone());
                    }
                    this.check_expr(&lambda.body, None);
                });
            }
            ExprKind::Let {
                pattern,
                binder,
                body,
            } => self.in_drop_scope(body.line, |this| {
                this.check_expr(binder,None);
                this.check_pattern(pattern);
                this.check_expr(body,None);
            }),
            ExprKind::Borrow { var_name,new_var,new_ty,region,body,.. } => {
                self.init_var(*new_var, var_name.content.clone(), new_ty.clone());
                self.check_expr(body,None);
                self.expired_regions.insert(*region);
                if self.ty_is_expired(&body.ty){
                    self.err.report(format!("Cannot let '{}' escape",body.ty), body.line);
                }
            },
            ExprKind::For {
                pattern,
                iterator,
                body,
            } => {
                self.in_drop_scope(pattern.line, |this|{
                    this.check_expr(iterator, None);
                    this.check_pattern(pattern);
                    this.check_expr(body, None);
                });

            },
            ExprKind::Case(..) => todo!("Handle case"),
        }
    }
    pub fn check_function(mut self, function: &Function) {
        self.region_params.extend(function.generics.iter().enumerate().filter_map(|(i,param)|{
            match param.kind {
                GenericKind::Region => Some(i),
                _ => None
            }
        }));
        self.in_drop_scope(function.body.line, |this|{
            for param in function.params.iter(){
                this.init_var(param.var, param.name.content.clone(), param.ty.clone());
            }
            this.check_expr(&function.body,None);
        });
        self.err.finish();
    }
}
