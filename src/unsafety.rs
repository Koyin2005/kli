use crate::{
    CtxtRef,
    resolved_ast::{AnnotationKind, Builtin, DefId},
    typed_ast::{ExprKind, Function, PlaceKind},
    typed_ast_visitor::{Visitor, walk_expr},
    types::{PointerType, Type},
};

pub fn transmutable(from: &Type, to: &Type) -> bool {
    match (from, to) {
        (Type::Byte, Type::Bool) | (Type::Bool, Type::Byte) => true,
        (Type::List(_), Type::String) | (Type::String, Type::List(_)) => true,
        _ => from.pointer_kind().is_some() && to.pointer_kind().is_some(),
    }
}

#[non_exhaustive]
pub struct SafetyCheckError;
pub fn is_unsafe(ctxt: CtxtRef, id: DefId) -> bool {
    ctxt.annotations(id)
        .iter()
        .any(|annotation| annotation.kind == AnnotationKind::Unsafe)
}
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
        if is_unsafe(ctxt, id) {
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
        let cause = match expr.kind {
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
                let id = self.ctxt.builtins().expect_id(builtin);
                if is_unsafe(self.ctxt, id) {
                    UnsafeCause::Function(id)
                } else {
                    return walk_expr(self, expr);
                }
            }
            ExprKind::Function(id, _) if is_unsafe(self.ctxt, id) => UnsafeCause::Function(id),
            ExprKind::Load(ref place)
                if let PlaceKind::Deref(ref value) = place.kind
                    && let Some(PointerType::Raw) = value.ty.pointer_kind() =>
            {
                UnsafeCause::RawDeref
            }
            _ => return walk_expr(self, expr),
        };
        self.ctxt.diag().add_diagnostic(
            match cause {
                UnsafeCause::Function(id) => {
                    format!(
                        "use of unsafe function '{}' outside unsafe context",
                        self.ctxt.expect_ident(id).symbol
                    )
                }
                UnsafeCause::RawDeref => "raw pointer deref outside unsafe context".to_string(),
            },
            expr.loc,
        );
        walk_expr(self, expr)
    }
}

enum UnsafeCause {
    RawDeref,
    Function(DefId),
}
