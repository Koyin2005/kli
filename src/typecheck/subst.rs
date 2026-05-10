use crate::{
    typecheck::infer::TypeInfer,
    typed_ast::{Expr, ExprKind, Pattern, PatternKind, Place, PlaceKind},
    types::{FunctionType, GenericArg, Region, Type},
};

pub struct TypeSubst<'a> {
    infer: &'a mut TypeInfer,
}
impl<'a> TypeSubst<'a> {
    pub fn new(infer: &'a mut TypeInfer) -> Self {
        Self { infer }
    }
    pub fn subst_type(&mut self, ty: &mut Type) {
        match ty {
            Type::Bool
            | Type::Int
            | Type::String
            | Type::Unit
            | Type::Unknown
            | Type::Char
            | Type::Param(..) => (),
            Type::Box(ty) | Type::Option(ty) | Type::List(ty) => self.subst_type(ty),
            Type::Imm(region, ty) | Type::Mut(region, ty) => {
                self.subst_region(region);
                self.subst_type(ty);
            }
            Type::Function(FunctionType {
                binder: _,
                resource: _,
                params,
                return_type,
            }) => {
                for param in params {
                    self.subst_type(param);
                }
                self.subst_type(return_type);
            }
            Type::Infer(var) => *ty = self.infer.simplify_type(Type::Infer(*var)),
        }
    }
    pub fn subst_region(&mut self, region: &mut Region) {
        match region {
            Region::Static
            | Region::Unknown
            | Region::Param(..)
            | Region::Local(..)
            | Region::Bound(..) => (),
            Region::Infer(var) => *region = self.infer.simplify_region(Region::Infer(*var)),
        }
    }
    pub fn subst_generic_arg(&mut self, arg: &mut GenericArg) {
        match arg {
            GenericArg::Region(region) => {
                self.subst_region(region);
            }
            GenericArg::Type(ty) => {
                self.subst_type(ty);
            }
        }
    }
    pub fn subst_pattern(&mut self, pattern: &mut Pattern) {
        match &mut pattern.kind {
            PatternKind::None | PatternKind::Bool(_) => (),
            PatternKind::Some(pattern) => self.subst_pattern(pattern),
            PatternKind::Deref(pattern) => self.subst_pattern(pattern),
            PatternKind::Binding(.., ty) => self.subst_type(ty),
        }
        self.subst_type(&mut pattern.ty);
    }
    pub fn subst_place(&mut self, place: &mut Place) {
        match &mut place.kind {
            PlaceKind::Deref(expr) => self.subst_expr(expr),
            PlaceKind::Var(..) => (),
        }
        self.subst_type(&mut place.ty);
    }
    pub fn subst_expr(&mut self, expr: &mut Expr) {
        match &mut expr.kind {
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::String(_)
            | ExprKind::Panic
            | ExprKind::None => (),
            ExprKind::Sequence(first, second) | ExprKind::Binary(_, first, second) => {
                self.subst_expr(first);
                self.subst_expr(second);
            }
            ExprKind::Print(expr) => {
                if let Some(expr) = expr {
                    self.subst_expr(expr);
                }
            }
            ExprKind::Some(expr) => self.subst_expr(expr),
            ExprKind::List(exprs) => {
                for expr in exprs {
                    self.subst_expr(expr);
                }
            }
            ExprKind::Call(callee, args) => {
                self.subst_expr(callee);
                for arg in args {
                    self.subst_expr(arg);
                }
            }
            ExprKind::Load(place) => {
                self.subst_place(place);
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
            } => {
                self.subst_pattern(pattern);
                self.subst_expr(iterator);
                self.subst_expr(body);
            }
            ExprKind::Assign(place, expr) => {
                self.subst_place(place);
                self.subst_expr(expr);
            }
            ExprKind::Let {
                pattern,
                binder,
                body,
            } => {
                self.subst_pattern(pattern);
                self.subst_expr(binder);
                self.subst_expr(body);
            }
            ExprKind::Borrow { new_ty, body, .. } => {
                self.subst_type(new_ty);
                self.subst_expr(body);
            }
            ExprKind::Case(matchee, arms) => {
                self.subst_expr(matchee);
                for arm in arms {
                    self.subst_pattern(&mut arm.pattern);
                    self.subst_expr(&mut arm.body);
                }
            }
            ExprKind::Builtin(_, args) | ExprKind::Function(.., args) => {
                for arg in args {
                    self.subst_generic_arg(arg);
                }
            }
            ExprKind::Lambda(lambda) => {
                for (.., ty) in lambda.params.iter_mut() {
                    self.subst_type(ty);
                }
                self.subst_type(&mut lambda.return_type);
                self.subst_expr(&mut lambda.body);
            }
        }
        self.subst_type(&mut expr.ty);
    }
}
