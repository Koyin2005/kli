use crate::{ast::{Expr, ExprKind}, typecheck::{root::TypeCheck, types::Type}};

impl TypeCheck{
    pub(super) fn check_expr(&mut self, expr: &Expr, expected_ty : Option<Type>) -> Type{
        match expr.kind{
            ExprKind::Unit => {
                if let Some(ty) = expected_ty {
                    self.unify(ty, Type::Unit, expr.line)
                }
                else {
                    Type::Unit
                }
            },
            _ => todo!("Handle checking {:?}",expr.kind)
        }
    }
}