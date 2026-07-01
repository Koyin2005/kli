use crate::{
    mir::{
        StmtKind,
        build::{Builder, expr::BuiltinResult},
    },
    typed_ast::{Expr, ExprKind},
};

impl Builder<'_> {
    pub(super) fn expr_stmt(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Err => (),
            ExprKind::Assign(place, value) => {
                let place = self.lower_place(place);
                let value = self.build_rvalue(value);
                self.assign(expr.loc, place, value);
            }
            ExprKind::Panic => {
                self.panic(expr.loc);
            }
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_stmt(&block_body.expr);
            }
            ExprKind::Print(value) => {
                let stmt = StmtKind::Print(value.as_ref().map(|expr| self.operand(expr)));
                self.push_stmt(expr.loc, stmt);
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
                iterator_type,
            } => {
                self.for_loop(pattern, iterator, iterator_type, body);
            }
            ExprKind::BuiltinCall(builtin, _, args) => {
                match self.builtin_call(expr.loc, &expr.ty, *builtin, args) {
                    BuiltinResult::Rvalue(value) => {
                        self.assign_to_temp(expr.loc, expr.ty.clone(), value);
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
            | ExprKind::List(..)
            | ExprKind::Lambda(..)
            | ExprKind::Const(..)
            | ExprKind::VariantInit(..)
            | ExprKind::AddressOf(..) => {
                self.expr_into_temp(expr);
            }
        }
    }
}
