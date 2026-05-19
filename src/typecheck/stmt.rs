use crate::{
    resolved_ast::{Stmt, StmtKind},
    typecheck::root::TypeCheck,
    typed_ast,
};

impl TypeCheck {
    pub(super) fn check_stmt(&mut self, stmt: Stmt) -> typed_ast::Stmt {
        let line = stmt.line;
        match stmt.kind {
            StmtKind::Expr(expr) => {
                let expr = self.check_expr(expr, None);
                typed_ast::Stmt {
                    line,
                    kind: typed_ast::StmtKind::Expr(expr),
                }
            }
            StmtKind::Let(let_binding) => {
                let let_binding = self.check_binding(let_binding);
                typed_ast::Stmt {
                    line,
                    kind: typed_ast::StmtKind::Let(let_binding),
                }
            }
        }
    }
}
