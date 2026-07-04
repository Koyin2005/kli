use crate::{
    resolved_ast::{Stmt, StmtKind},
    typecheck::root::FunctionCtxt,
    typed_ast,
};

impl FunctionCtxt<'_> {
    pub(super) fn check_stmt(&self, stmt: &Stmt) -> typed_ast::Stmt {
        let loc = stmt.loc;
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                let expr = self.check_expr(expr, None);
                typed_ast::Stmt {
                    loc,
                    kind: typed_ast::StmtKind::Expr(expr),
                }
            }
            StmtKind::Let(let_binding) => {
                let let_binding = self.check_binding(let_binding);
                typed_ast::Stmt {
                    loc,
                    kind: typed_ast::StmtKind::Let(let_binding),
                }
            }
        }
    }
}
