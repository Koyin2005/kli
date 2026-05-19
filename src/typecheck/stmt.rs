use crate::{resolved_ast::Stmt, typecheck::root::TypeCheck, typed_ast};


impl TypeCheck{
    pub(super) fn check_stmt(&mut self, stmt: Stmt) -> typed_ast::Stmt{
        let line = stmt.line;
        match stmt.kind{

        }
    }
}