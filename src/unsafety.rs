use crate::{
    CtxtRef,
    resolved_ast::{AnnotationKind, Builtin, DefId},
    typed_ast::{ExprKind, Function},
    typed_ast_visitor::{Visitor, walk_expr},
    types::Type,
};

pub fn transmutable(from: &Type, to: &Type) -> bool {
    from.pointer_kind().is_some() && to.pointer_kind().is_some()
}

#[non_exhaustive]
pub struct SafetyCheckError;
pub struct SafetyCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> SafetyCheck<'ctxt> {
    pub fn check(
        ctxt: CtxtRef<'ctxt>,
        id: DefId,
        function: &Function,
    ) -> Result<(), SafetyCheckError> {
        if ctxt
            .std_lib_module()
            .is_some_and(|std| ctxt.ancestors(id).any(|id| id == std))
        {
            return Ok(());
        }
        let mut this = Self { ctxt };
        if let Some(body) = function.body.as_ref() {
            this.visit_expr(body);
        }
        if !this.ctxt.diag().report_all() {
            Ok(())
        } else {
            Err(SafetyCheckError)
        }
    }
}
impl Visitor for SafetyCheck<'_> {
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr) {
        let id = match expr.kind {
            ExprKind::BuiltinCall(builtin, ref args, _) => {
                if let Builtin::Transmute = builtin
                    && let Some(
                        [
                            crate::types::GenericArg::Type(ty1),
                            crate::types::GenericArg::Type(ty2),
                        ],
                    ) = &args.as_array()
                    && !transmutable(ty1, ty2)
                {
                    self.ctxt.diag().add_diagnostic(
                        format!("cannot transmute from '{}' to '{}'", ty1, ty2),
                        expr.loc,
                    );
                }
                self.ctxt.builtins().expect_id(builtin)
            }
            ExprKind::Function(id, _) => id,
            _ => return walk_expr(self, expr),
        };
        let is_unsafe = self
            .ctxt
            .expect_item(id)
            .annotations
            .iter()
            .any(|annotation| annotation.kind == AnnotationKind::Unsafe);
        if is_unsafe {
            self.ctxt.diag().add_diagnostic(
                format!(
                    "use of unsafe function '{}' outside unsafe context",
                    self.ctxt.name(id).symbol
                ),
                expr.loc,
            );
        }
        walk_expr(self, expr)
    }
}
