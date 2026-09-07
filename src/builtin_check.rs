use crate::{
    CtxtRef,
    builtins::{Builtin, IntegerBuiltin},
    src_loc::SrcLoc,
    typed_ast::{ExprKind, Function},
    typed_ast_visitor::{Visitor, walk_expr},
    types::GenericArgsRef,
};

pub struct BuiltinCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    errored: bool,
}

impl<'ctxt> BuiltinCheck<'ctxt> {
    pub fn check(ctxt: CtxtRef<'ctxt>, function: &Function<'ctxt>) -> bool {
        let mut check = Self {
            ctxt,
            errored: false,
        };
        if let Some(ref expr) = function.body {
            check.visit_expr(expr);
        }
        check.errored
    }

    fn check_builtin(
        &mut self,
        loc: SrcLoc,
        builtin: Builtin,
        generic_args: GenericArgsRef<'_, 'ctxt>,
    ) {
        let error = match builtin {
            Builtin::IntegerBuiltin(integer_builtin) => match integer_builtin {
                IntegerBuiltin::IntMaxValue
                | IntegerBuiltin::ShiftLeft
                | IntegerBuiltin::ShiftRight
                | IntegerBuiltin::OverflowingAdd
                | IntegerBuiltin::OverflowingSub
                | IntegerBuiltin::WrappingAdd
                | IntegerBuiltin::WrappingSub
                | IntegerBuiltin::OverflowingMul
                | IntegerBuiltin::WrappingMul => {
                    let ty = generic_args[0].expect_ty();
                    (!ty.is_integer()).then(|| {
                        format!(
                            "cannot call '{}' with non-integer type '{}'",
                            builtin.name(),
                            ty
                        )
                    })
                }
            },
            _ => None,
        };
        if let Some(error) = error {
            self.ctxt.diag().add_diagnostic(error, loc);
            self.errored = true;
        }
    }
}

impl<'ctxt> Visitor<'ctxt> for BuiltinCheck<'ctxt> {
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr<'ctxt>) {
        if let &ExprKind::BuiltinCall(builtin, ref generic_args, _) = &expr.kind {
            self.check_builtin(expr.loc, builtin, generic_args);
        }
        walk_expr(self, expr);
    }
}
