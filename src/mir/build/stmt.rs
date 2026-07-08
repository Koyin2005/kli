use crate::{
    mir::{
        Place, StmtKind, TerminatorKind, build::{Builder, expr::BuiltinResult},
    }, typed_ast::{Expr, ExprKind},
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
            ExprKind::Return(value) => {
                self.expr_into_dest(Place::return_place(), value);
                self.finish_block(expr.loc, TerminatorKind::Return);
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
            ExprKind::While(condition, body) => {
                // while cond body
                // L1
                //  if cond goto L2 else goto L3
                // L2
                //  body
                //  goto L1
                // L3
                let loop_start = self.goto_to_new_block(condition.loc);
                let condition = self.operand(condition);

                let body_start_block = self.switch_to_new_block();
                self.expr_stmt(body);
                self.finish_block_with_goto(expr.loc, loop_start);

                let end_block = self.new_block();
                self.switch_to_block(loop_start);
                self.finish_block_with_if(expr.loc, condition, body_start_block, end_block);

                self.switch_to_block(end_block);
            }
            ExprKind::BuiltinCall(builtin, _, args) => {
                match self.builtin_call(expr.loc, &expr.ty, *builtin, args) {
                    BuiltinResult::Rvalue(value) => {
                        self.assign_to_temp(expr.loc, expr.ty.clone(), value);
                    }
                    BuiltinResult::Unit => (),
                }
            }
            ExprKind::NeverToAny(value) => {
                self.expr_stmt(value);
                self.finish_block(expr.loc, TerminatorKind::Unreachable);
                self.switch_to_new_block();
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
            | ExprKind::Lambda(..)
            | ExprKind::Const(..)
            | ExprKind::VariantInit(..)
            | ExprKind::AddressOf(..)
            | ExprKind::NamedRecord(..)
            | ExprKind::Logic(..) => {
                self.expr_into_temp(expr);
            }
        }
    }
}
