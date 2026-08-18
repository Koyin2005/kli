use crate::{
    CtxtRef,
    builtins::{Builtin, IntegerBuiltin},
    src_loc::SrcLoc,
    typed_ast::{ExprKind, Function},
    typed_ast_visitor::{Visitor, walk_expr},
    types::{GenericArg, GenericArgsRef, IntegerKind, IntegerSize, TypeKind},
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
        let error = match builtin {
            Builtin::Bitcast => {
                let [from, to] = generic_args.as_array().unwrap().map(GenericArg::expect_ty);
                let is_valid_bitcast = matches!(
                    (from.kind(), to.kind()),
                    (
                        TypeKind::Bool,
                        TypeKind::Int(
                            IntegerKind::Signed(IntegerSize::Int8)
                                | IntegerKind::Unsigned(IntegerSize::Int8),
                        ),
                    ) | (TypeKind::Char, TypeKind::Int(IntegerKind::UINT32))
                ) || from
                    .as_integer()
                    .and_then(|from| to.as_integer().map(|to| (from, to)))
                    .is_some_and(|(from, to)| from.size() == to.size());

                (!is_valid_bitcast).then(|| format!("cannot bitcast from '{}' to '{}'", from, to))
            }
            Builtin::Transmute => {
                let [from, to] = generic_args.as_array().unwrap().map(GenericArg::expect_ty);
                let valid_transmute = unsafety::transmutable(self.ctxt, from, to);
                (!valid_transmute).then(|| format!("cannot transmute from '{}' to '{}'", from, to))
            }
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
                IntegerBuiltin::Widen => {
                    let [from, to] = generic_args.as_array().unwrap().map(GenericArg::expect_ty);
                    let from_int = from.as_integer();
                    let to_int = to.as_integer();

                    let can_widen = match (from_int, to_int) {
                        (
                            Some(IntegerKind::Signed(from_size)),
                            Some(IntegerKind::Signed(to_size)),
                        ) => from_size.bit_width() < to_size.bit_width(),
                        (
                            Some(IntegerKind::Unsigned(from_size)),
                            Some(IntegerKind::Unsigned(to_size)),
                        ) => from_size.bit_width() < to_size.bit_width(),
                        (Some(IntegerKind::UINT8), None) => to.is_char(),
                        _ => false,
                    };

                    (!can_widen).then(|| format!("cannot widen '{}' to '{}'", from, to))
                }
                IntegerBuiltin::Truncate => {
                    let [from, to] = generic_args.as_array().unwrap().map(GenericArg::expect_ty);
                    let from_int = from.as_integer();
                    let to_int = to.as_integer();
                    let valid_truncate = match (from_int, to_int) {
                        (
                            Some(IntegerKind::Signed(from_size)),
                            Some(IntegerKind::Signed(to_size)),
                        ) => from_size.bit_width() > to_size.bit_width(),
                        (
                            Some(IntegerKind::Unsigned(from_size)),
                            Some(IntegerKind::Unsigned(to_size)),
                        ) => from_size.bit_width() > to_size.bit_width(),
                        (None, Some(IntegerKind::UINT8)) => from.is_char(),
                        _ => false,
                    };
                    (!valid_truncate).then(|| format!("cannot truncate '{}' to '{}'", from, to))
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
        if let &ExprKind::BuiltinCall(_, builtin, ref generic_args, _) = &expr.kind {
            self.check_builtin(expr.loc, builtin, generic_args);
        }
        walk_expr(self, expr);
    }
}
