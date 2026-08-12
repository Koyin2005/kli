use crate::{CtxtRef, typed_ast, typed_ast_visitor::Visitor};

pub struct LiteralCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    had_errors: bool,
}
impl<'ctxt> LiteralCheck<'ctxt> {
    pub fn check(ctxt: CtxtRef<'ctxt>, function: &typed_ast::Function<'ctxt>) -> bool {
        let mut check = Self {
            ctxt,
            had_errors: false,
        };

        if let Some(ref body) = function.body {
            check.visit_expr(body);
        }
        check.had_errors
    }
}

impl<'ctxt> Visitor<'ctxt> for LiteralCheck<'ctxt> {
    fn visit_lit(&mut self, loc: crate::src_loc::SrcLoc, lit: u64, ty: crate::types::Type<'ctxt>) {
        let ty = ty.as_integer().unwrap();
        if lit as i128 > ty.max_value_scalar() {
            self.ctxt
                .diag()
                .add_diagnostic(format!("{lit} is too large for '{ty}'"), loc);
            self.had_errors = true;
        }
    }
}
