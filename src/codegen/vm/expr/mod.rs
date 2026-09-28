use crate::{
    codegen::vm::{
        CodegenFunction, CodegenPlace, PlaceRepr, RegWindow, Repr, ReprKind, SCALAR_REPR,
    },
    index_vec::IndexVec,
    ir::{self, BinaryOp},
    typed_ast::FieldId,
    vm::instructions::{self, FunctionId},
};
pub enum CallArg {
    Scalar(i64),
    Reg(RegWindow),
}
pub enum Callee {
    Function(FunctionId),
    Reg(instructions::Reg),
}
pub enum Conditional {
    Bool(bool),
    Reg(instructions::Reg),
    Not(instructions::Reg),
    GtEq(instructions::Reg, instructions::Reg),
    Lt(instructions::Reg, instructions::Reg),
}
impl CodegenFunction<'_> {
    pub(super) fn codegen_binary_op(
        &mut self,
        dst: RegWindow,
        op: ir::BinaryOp,
        left: instructions::Reg,
        right: instructions::Reg,
    ) {
        let op = match op {
            ir::BinaryOp::Add => instructions::Instr::Add {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::AddWithOverflow => todo!(),
            ir::BinaryOp::Subtract => instructions::Instr::Sub {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::SubtractWithOverflow => todo!(),
            ir::BinaryOp::Multiply => instructions::Instr::Mul {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::MultiplyWithOverflow => todo!(),
            ir::BinaryOp::Divide => instructions::Instr::Div {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::Lesser => instructions::Instr::LesserThan {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::Greater => todo!(),
            ir::BinaryOp::Equals => instructions::Instr::Equals {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::BitwiseAnd => todo!(),
            ir::BinaryOp::BitwiseOr => todo!(),
            ir::BinaryOp::InBounds => todo!(),
        };
        self.push_instr(op);
    }

    pub(super) fn codegen_copy(&mut self, dst: CodegenPlace, src: CodegenPlace) {
        match (dst, src) {
            (CodegenPlace::Reg(dst), CodegenPlace::Reg(src)) => {
                for (dst, src) in dst.into_iter().zip(src.into_iter()) {
                    self.push_instr(instructions::Instr::Move { dst, src });
                }
            }
        }
    }

    pub(super) fn codegen_expr_into_reg_window(
        &mut self,
        expr: &ir::Expr,
        repr: Repr,
    ) -> RegWindow {
        match &expr.kind {
            ir::ExprKind::Load(place) => match self.lower_codegen_place(place).0 {
                CodegenPlace::Reg(reg_window) => reg_window,
            },
            _ => {
                let temp_place = self.temp_place(repr.size_as_u16());
                self.codegen_expr_into(
                    expr,
                    PlaceRepr {
                        place: CodegenPlace::Reg(temp_place),
                        repr,
                    },
                );
                temp_place
            }
        }
    }
    pub(super) fn temp_place(&mut self, size: u16) -> RegWindow {
        let base = self.reserve_registers(size);
        RegWindow { base, size }
    }

    fn simplify_binary_op(
        &self,
        op: ir::BinaryOp,
        left: &ir::Expr,
        right: &ir::Expr,
    ) -> Option<ir::Expr> {
        let left_const = left.as_constant();
        let right_const = right.as_constant();
        match op {
            ir::BinaryOp::Equals => Some(ir::Expr::constant_bool(left_const? == right_const?)),
            ir::BinaryOp::Divide => {
                let &ir::Constant::Int(right) = right_const? else {
                    return None;
                };
                if right == 0 {
                    return None;
                }
                let &ir::Constant::Int(left) = left_const? else {
                    return if right == 1 { Some(left.clone()) } else { None };
                };
                Some(ir::Expr::constant_int(left.wrapping_div(right)))
            }
            ir::BinaryOp::Add => match (left_const, right_const) {
                (Some(ir::Constant::Int(left)), Some(ir::Constant::Int(right))) => {
                    Some(ir::Expr::constant_int((*left).wrapping_add(*right)))
                }
                (Some(ir::Constant::Int(0)), None) => Some(right.clone()),
                (None, Some(ir::Constant::Int(0))) => Some(left.clone()),
                _ => None,
            },

            _ => None,
        }
    }
    fn codegen_aggregrate(
        &mut self,
        place: PlaceRepr,
        aggregrate: &ir::AggregateKind,
        fields: &IndexVec<FieldId, ir::Expr>,
    ) {
        let CodegenPlace::Reg(window) = place.place;
        match aggregrate {
            ir::AggregateKind::Tuple => {
                let ReprKind::Tuple(field_reprs) = place.repr.kind else {
                    unreachable!()
                };
                let mut offset = 0u16;
                for (field, field_repr) in fields.iter().zip(field_reprs) {
                    let size = field_repr.size_as_u16();
                    let base = window.offset_by(offset).base;
                    self.codegen_expr_into(
                        field,
                        PlaceRepr {
                            place: CodegenPlace::Reg(RegWindow { base, size }),
                            repr: field_repr.clone(),
                        },
                    );
                    offset += size;
                }
            }
            ir::AggregateKind::Named => todo!(),
            &ir::AggregateKind::Variant(_, case_id, _) => {
                let ReprKind::Tuple(field_reprs) = place.repr.kind else {
                    unreachable!()
                };

                let [_, union] = field_reprs.into_vec().try_into().unwrap();
                let ReprKind::Union(case_reprs) = union.kind else {
                    unreachable!()
                };
                let ReprKind::Tuple(field_reprs) = { case_reprs }
                    .into_vec()
                    .swap_remove((case_id).into_usize())
                    .kind
                else {
                    unreachable!()
                };
                self.load_immediate(window.base, case_id.into_u32().into());
                let mut offset = 1;
                for (field, field_repr) in fields.iter().zip(field_reprs) {
                    let size = field_repr.size_as_u16();
                    let base = window.offset_by(offset).base;
                    self.codegen_expr_into(
                        field,
                        PlaceRepr {
                            place: CodegenPlace::Reg(RegWindow { base, size }),
                            repr: field_repr.clone(),
                        },
                    );
                    offset += size;
                }
            }
        }
    }
    pub(super) fn codegen_expr_into(&mut self, expr: &ir::Expr, place: PlaceRepr) {
        match &expr.kind {
            ir::ExprKind::Constant(constant) => {
                let value = self.eval_imm_constant(constant);
                self.codegen_imm_store(place.place, value);
            }
            ir::ExprKind::Load(src) => {
                let (src_place, _) = self.lower_codegen_place(src);
                self.codegen_copy(place.place, src_place);
            }
            ir::ExprKind::Len(place) => todo!(),
            ir::ExprKind::Discriminant(place) => todo!(),
            ir::ExprKind::Aggregate(aggregate_kind, fields) => {
                self.codegen_aggregrate(place, aggregate_kind, fields)
            }
            &ir::ExprKind::BinaryOp(op, ref left, ref right) => {
                if let Some(result) = self.simplify_binary_op(op, left, right) {
                    return self.codegen_expr_into(&result, place);
                }
                let dst = {
                    let CodegenPlace::Reg(reg) = place.place;
                    reg
                };
                if let BinaryOp::Add = op {
                    match (left.as_constant(), right.as_constant()) {
                        (Some(&ir::Constant::Int(value)), None)
                            if let Ok(value) = value.try_into() =>
                        {
                            let right = self.codegen_expr_into_reg_window(right, SCALAR_REPR).base;
                            self.add_imm(dst.base, right, value);
                            return;
                        }
                        (None, Some(&ir::Constant::Int(value)))
                            if let Ok(value) = value.try_into() =>
                        {
                            let left = self.codegen_expr_into_reg_window(left, SCALAR_REPR).base;
                            self.add_imm(dst.base, left, value);
                            return;
                        }
                        _ => (),
                    }
                }
                let left = self.codegen_expr_into_reg_window(left, SCALAR_REPR).base;
                let right = self.codegen_expr_into_reg_window(right, SCALAR_REPR).base;
                self.codegen_binary_op(dst, op, left, right);
            }
            ir::ExprKind::Not(expr) => {
                let dst = {
                    let CodegenPlace::Reg(reg) = place.place;
                    reg
                }
                .base;
                let src = self.codegen_expr_into_reg_window(expr, SCALAR_REPR).base;
                self.push_instr(instructions::Instr::Not { dst, src });
            }
        }
    }

    pub(super) fn lower_condition(&mut self, expr: &ir::Expr) -> Conditional {
        match &expr.kind {
            ir::ExprKind::Constant(ir::Constant::Bool(value)) => Conditional::Bool(*value),
            ir::ExprKind::Not(value) => match self.lower_condition(value) {
                Conditional::Bool(value) => Conditional::Bool(!value),
                Conditional::Reg(reg) => Conditional::Not(reg),
                Conditional::Not(reg) => Conditional::Reg(reg),
                Conditional::GtEq(left, right) => Conditional::Lt(left, right),
                Conditional::Lt(left, right) => Conditional::GtEq(left, right),
            },
            ir::ExprKind::BinaryOp(op, left, right)
                if let Some(expr) = self.simplify_binary_op(*op, left, right) =>
            {
                self.lower_condition(&expr)
            }
            ir::ExprKind::BinaryOp(ir::BinaryOp::Equals, left, right)
                if let Some(ir::Constant::Int(0) | ir::Constant::Bool(false)) =
                    right.as_constant() =>
            {
                let left = self.codegen_expr_into_reg_window(left, SCALAR_REPR).base;
                return Conditional::Not(left);
            }
            ir::ExprKind::BinaryOp(ir::BinaryOp::Lesser, left, right) => {
                let left = self.codegen_expr_into_reg_window(left, SCALAR_REPR).base;
                let right = self.codegen_expr_into_reg_window(right, SCALAR_REPR).base;
                return Conditional::Lt(left, right);
            }
            _ => Conditional::Reg({
                let reg = self.codegen_expr_into_reg_window(expr, SCALAR_REPR).base;
                reg
            }),
        }
    }
    pub(super) fn lower_callee(&mut self, expr: &ir::Expr) -> Callee {
        match &expr.kind {
            &ir::ExprKind::Constant(ir::Constant::Function(id, ref args)) => {
                Callee::Function(self.function_id(id, args.clone()))
            }
            _ => {
                let reg = self.codegen_expr_into_reg_window(expr, SCALAR_REPR).base;
                Callee::Reg(reg)
            }
        }
    }
    pub(super) fn lower_call_arg(&mut self, expr: &ir::Expr, repr: Repr, args: &mut Vec<CallArg>) {
        match &expr.kind {
            ir::ExprKind::Constant(constant) => {
                args.push(CallArg::Scalar(self.eval_imm_constant(constant)));
            }
            ir::ExprKind::Aggregate(kind, fields) => match kind {
                ir::AggregateKind::Tuple | ir::AggregateKind::Named => {
                    for arg in fields {
                        let repr = self.codegen.type_repr(
                            &arg.type_of(self.program, &self.program.bodies[self.id]),
                            self.program,
                            &self.args,
                        );
                        self.lower_call_arg(arg, repr, args);
                    }
                }
                ir::AggregateKind::Variant(_, id, _) => {
                    args.push(CallArg::Scalar(id.into_u32().into()));
                    for arg in fields {
                        let repr = self.codegen.type_repr(
                            &arg.type_of(self.program, &self.program.bodies[self.id]),
                            self.program,
                            &self.args,
                        );
                        self.lower_call_arg(arg, repr, args);
                    }
                }
            },
            _ => {
                let reg = self.codegen_expr_into_reg_window(expr, repr);
                args.push(CallArg::Reg(reg));
            }
        }
    }
}
