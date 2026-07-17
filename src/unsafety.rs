use crate::{
    CtxtRef,
    builtins::Builtin,
    def_ids::DefId,
    lang_items::LangItem,
    resolved_ast::AnnotationKind,
    typed_ast::{ExprKind, Function, PlaceKind},
    typed_ast_visitor::{Visitor, walk_expr, walk_place},
    types::Type,
};

pub fn transmutable(ctxt: CtxtRef<'_>, from: &Type, to: &Type) -> bool {
    match (from, to) {
        (from, to) if from == to => true,
        (Type::Int(_), Type::Int(_)) => true,
        (Type::Byte, Type::Bool) | (Type::Bool, Type::Byte) => true,
        (&Type::Named(id, _, _), &Type::Named(id2, _, _))
            if let lang_items = ctxt.lang_items()
                && let Some(string_id) = lang_items.get(LangItem::String)
                && let Some(list_id) = lang_items.get(LangItem::ArrayList)
                && ((id == string_id && id2 == list_id) | (id == list_id && id2 == string_id)) =>
        {
            true
        }
        _ => from.pointer_kind(ctxt).is_some() && to.pointer_kind(ctxt).is_some(),
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
    in_unsafe_block: bool,
}
impl<'ctxt> SafetyCheck<'ctxt> {
    pub fn check(
        ctxt: CtxtRef<'ctxt>,
        id: DefId,
        function: &Function,
    ) -> Result<(), SafetyCheckError> {
        if is_unsafe(ctxt, id) {
            return Ok(());
        }
        let mut this = Self {
            ctxt,
            in_unsafe_block: false,
        };
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
    fn visit_place(&mut self, place: &crate::typed_ast::Place) {
        if self.in_unsafe_block {
            return;
        }
        if let PlaceKind::Deref(ref value) = place.kind
            && value.ty.as_pointer().is_some()
        {
            self.ctxt.diag().add_diagnostic(
                "deref of raw pointer outside unsafe context".to_string(),
                place.loc,
            )
        }
        walk_place(self, place);
    }
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr) {
        if self.in_unsafe_block {
            return;
        }
        let function = match expr.kind {
            ExprKind::Unsafe(ref expr) => {
                let was_in_unsafe_block = self.in_unsafe_block;
                self.in_unsafe_block = true;
                self.visit_expr(expr);
                self.in_unsafe_block = was_in_unsafe_block;
                return;
            }
            ExprKind::BuiltinCall(builtin, ref args, _) => {
                if let Builtin::Transmute = builtin
                    && let Some(
                        [
                            crate::types::GenericArg::Type(ty1),
                            crate::types::GenericArg::Type(ty2),
                        ],
                    ) = &args.as_array()
                    && !transmutable(self.ctxt, ty1, ty2)
                {
                    self.ctxt.diag().add_diagnostic(
                        format!("cannot transmute from '{}' to '{}'", ty1, ty2),
                        expr.loc,
                    );
                }
                let id = self.ctxt.builtins().expect_id(builtin);
                if is_unsafe(self.ctxt, id) {
                    id
                } else {
                    return walk_expr(self, expr);
                }
            }
            ExprKind::Function(id, _) if is_unsafe(self.ctxt, id) => id,
            _ => return walk_expr(self, expr),
        };
        self.ctxt.diag().add_diagnostic(
            format!(
                "use of unsafe function '{}' outside unsafe context",
                self.ctxt.expect_ident(function).symbol
            ),
            expr.loc,
        )
    }
}
