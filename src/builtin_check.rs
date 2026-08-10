use crate::{
    CtxtRef,
    builtins::{Builtin, IntegerBuiltin},
    src_loc::SrcLoc,
    typed_ast::{ExprKind, Function},
    typed_ast_visitor::{Visitor, walk_expr},
    types::{GenericArg, GenericArgsRef, IntegerKind},
    unsafety,
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
        self.errored |= match builtin {
            Builtin::Transmute => {
                let [from, to] = generic_args.as_array().unwrap().map(GenericArg::expect_ty);
                if !unsafety::transmutable(self.ctxt, from, to) {
                    self.ctxt.diag().add_diagnostic(
                        format!("cannot transmute from '{}' to '{}'", from, to),
                        loc,
                    );
                    true
                } else {
                    false
                }
            }
            Builtin::IntegerBuiltin(integer_builtin) => match integer_builtin {
                IntegerBuiltin::IntMaxValue
                | IntegerBuiltin::ShiftLeft
                | IntegerBuiltin::ShiftRight
                | IntegerBuiltin::OverflowingAdd
                | IntegerBuiltin::OverflowingSub
                | IntegerBuiltin::WrappingAdd
                | IntegerBuiltin::WrappingSub => {
                    let ty = generic_args[0].expect_ty();
                    if !ty.is_integer() {
                        self.ctxt.diag().add_diagnostic(
                            format!(
                                "cannot call '{}' with non-integer type '{}'",
                                builtin.name(),
                                ty
                            ),
                            loc,
                        );
                        true
                    } else {
                        false
                    }
                }
                IntegerBuiltin::ZeroExtend => {
                    let [from, to] = generic_args.as_array().unwrap().map(GenericArg::expect_ty);
                    let from_int = from.as_integer();
                    let to_int = to.as_integer();
                    match (from_int, to_int) {
                        (
                            Some(IntegerKind::Signed(from_size)),
                            Some(IntegerKind::Signed(to_size)),
                        ) if from_size.bit_width() < to_size.bit_width() => false,
                        (
                            Some(IntegerKind::Unsigned(from_size)),
                            Some(IntegerKind::Unsigned(to_size)),
                        ) if from_size.bit_width() < to_size.bit_width() => false,
                        (Some(IntegerKind::UINT8), None) if to.is_char() => false,
                        _ => {
                            self.ctxt.diag().add_diagnostic(
                                format!("cannot zero extend '{}' to '{}'", from, to),
                                loc,
                            );
                            true
                        }
                    }
                }
            },
            _ => false,
        };
    }
}

impl<'ctxt> Visitor<'ctxt> for BuiltinCheck<'ctxt> {
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr<'ctxt>) {
        match expr.kind {
            ExprKind::BuiltinCall(_, builtin, ref generic_args, _) => {
                self.check_builtin(expr.loc, builtin, generic_args);
            }
            _ => (),
        }
        walk_expr(self, expr);
    }
}
