use crate::{
    mir::{
        Stmt,
        build::{Builder, expr::BuiltinResult},
    },
    typed_ast::{Expr, ExprKind},
};

impl Builder<'_> {
    pub(super) fn expr_stmt(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Err => (),
            ExprKind::Assign(place, expr) => {
                let place = self.lower_place(place);
                let value = self.build_rvalue(expr);
                self.assign(place, value);
            }
            ExprKind::Panic => {
                self.panic();
            }
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_stmt(&block_body.expr);
            }
            ExprKind::Print(expr) => {
                let stmt = Stmt::Print(expr.as_ref().map(|expr| self.operand(expr)));
                self.push_stmt(stmt);
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
                iterator_type,
            } => {
                self.for_loop(pattern, iterator, iterator_type, body);
            }
            ExprKind::BuiltinCall(builtin, generic_args, args) => {
                match self.builtin_call(&expr.ty, *builtin, generic_args, args) {
                    BuiltinResult::Rvalue(value) => {
                        self.assign_to_temp(expr.ty.clone(), value);
                    }
                    BuiltinResult::Unit => (),
                }
            }
            //Evaluate
            ExprKind::Record(..)
            | ExprKind::String(_)
            | ExprKind::Unit
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Borrow { .. }
            | ExprKind::Load(_)
            | ExprKind::Case(..)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
            | ExprKind::Function(..)
            | ExprKind::None
            | ExprKind::Some(..)
            | ExprKind::List(..)
            | ExprKind::Lambda(..) => {
                self.expr_into_temp(expr);
            }
        }
    }
}
