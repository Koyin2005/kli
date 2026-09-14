use crate::{
    mir::{
        TerminatorKind,
        build::{Builder, expr::BuiltinResult},
    },
    typed_ast::{Expr, ExprKind},
};

impl<'ctxt, 'mir> Builder<'mir, 'ctxt> {
    pub(super) fn expr_stmt(&'_ mut self, expr: &'_ Expr<'ctxt>) {
        match &expr.kind {
            ExprKind::Err => (),
            ExprKind::Assign(place, value) => {
                let place = self.lower_place(place);
                let value = self.expr_value(value);
                self.push_stmt(expr.loc, crate::mir::StmtKind::Store(place, value));
            }
            ExprKind::Panic => {
                self.panic(expr.loc);
            }
            ExprKind::Return(value) => {
                let return_value = self.expr_value(value);
                self.finish_block(expr.loc, TerminatorKind::Return(return_value));
                self.switch_to_new_block();
            }
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_stmt(&block_body.expr);
            }
            ExprKind::Unsafe(expr) => {
                self.expr_stmt(expr);
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
                let loop_start = self.goto_to_new_block(expr.loc);
                let loop_condition = self.expr_value(condition);
                let loop_cond_end = self.current_block;

                let loop_body = self.switch_to_new_block();
                self.expr_stmt(body);
                let loop_body_end = self.current_block;

                let end = self.new_block();
                self.switch_to_block(loop_cond_end);
                self.finish_block_with_if(expr.loc, loop_condition, loop_body, end);

                self.switch_to_block(loop_body_end);
                self.finish_block_with_goto(expr.loc, loop_start);

                self.switch_to_block(end);
            }
            ExprKind::BuiltinCall(builtin, _, args) => {
                match self.builtin_call(expr.loc, *builtin, args) {
                    BuiltinResult::Rvalue(value) => {
                        self.assign_to_temp(expr.loc, expr.ty, value);
                    }
                }
            }
            ExprKind::NeverToAny(value) => {
                self.expr_stmt(value);
                self.finish_block(expr.loc, TerminatorKind::Unreachable);
                self.switch_to_new_block();
            }
            //Evaluate
            ExprKind::String(_)
            | ExprKind::Unit
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Load(_)
            | ExprKind::Case(..)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
            | ExprKind::Function(..)
            | ExprKind::Lambda(..)
            | ExprKind::VariantInit(..)
            | ExprKind::NamedRecord(..)
            | ExprKind::Logic(..)
            | ExprKind::Tuple(..)
            | ExprKind::Array(..)
            | ExprKind::Char(_) => {
                self.expr_value(expr);
            }
        }
    }
}
