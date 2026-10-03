use crate::{
    codegen::vm::{CodegenFunction, CodegenPlace, PlaceRepr, RegWindow, Repr, SCALAR_REPR},
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
            ir::BinaryOp::Greater => instructions::Instr::GreaterThan {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::Equals => instructions::Instr::Equals {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::BitwiseAnd => instructions::Instr::And {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::BitwiseOr => instructions::Instr::Or {
                dst: dst.base,
                src1: left,
                src2: right,
            },
            ir::BinaryOp::InBounds => instructions::Instr::LesserThanUnsigned {
                dst: dst.base,
                src1: left,
                src2: right,
            },
        };
        self.push_instr(op);
    }

    pub(super) fn codegen_copy(&mut self, dst: CodegenPlace, src: CodegenPlace, size: u16) {
        match (dst, src) {
            (CodegenPlace::Reg(dst), CodegenPlace::Reg(src)) => {
                for i in 0..size {
                    let dst = dst.offset_by(i);
                    let src = src.offset_by(i);
                    self.push_instr(instructions::Instr::Move { dst, src });
                }
            }
            (CodegenPlace::Reg(dst), CodegenPlace::Offset(base, index)) => {
                for i in 0..size {
                    let dst = dst.offset_by(i);
                    self.push_instr(instructions::Instr::Load {
                        dst,
                        src: instructions::Addr {
                            base,
                            offset: index as u32 + i as u32,
                        },
                    });
                }
            }
            (CodegenPlace::Offset(dst, offset), CodegenPlace::Reg(src)) => {
                for i in 0..size {
                    let src = src.offset_by(i);
                    self.push_instr(instructions::Instr::Store {
                        dst: instructions::Addr {
                            base: dst,
                            offset: offset as u32 + i as u32,
                        },
                        src,
                    });
                }
            }
            (CodegenPlace::Offset(dst, dst_offset), CodegenPlace::Offset(src, src_offset)) => {
                self.push_instr(instructions::Instr::Copy {
                    dst: instructions::Addr {
                        base: dst,
                        offset: dst_offset as u32,
                    },
                    src: instructions::Addr {
                        base: src,
                        offset: src_offset as u32,
                    },
                    count: size as u32,
                });
            }
        }
    }

    pub(super) fn load_to_regs(
        &mut self,
        base: instructions::Reg,
        offset: u32,
        size: u16,
    ) -> RegWindow {
        let reg = self.reserve_registers(size);
        for i in 0..size {
            self.push_instr(instructions::Instr::Load {
                dst: instructions::Reg::new(reg.into_u16() + i),
                src: instructions::Addr {
                    base,
                    offset: (offset + i as u32),
                },
            });
        }
        RegWindow { base: reg, size }
    }
    pub(super) fn expr_as_regs(&mut self, expr: &ir::Expr, repr: Repr) -> RegWindow {
        match &expr.kind {
            ir::ExprKind::Load(place) => match self.lower_place(place) {
                (CodegenPlace::Reg(base), _) => RegWindow {
                    base,
                    size: repr.size_as_u16(),
                },
                (CodegenPlace::Offset(base, index), repr) => {
                    self.load_to_regs(base, index, repr.size_as_u16())
                }
            },
            _ => {
                let temp_place = self.temp_place(repr.size_as_u16());
                self.expr_into_regs(expr, temp_place, repr);
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
        place_repr: PlaceRepr,
        aggregrate: &ir::AggregateKind,
        fields: &IndexVec<FieldId, ir::Expr>,
    ) {
        match aggregrate {
            ir::AggregateKind::Tuple | ir::AggregateKind::Record(..) => {
                for (id, field) in fields.iter_enumerated() {
                    let (place, field_repr) =
                        self.project_field(place_repr.place.clone(), place_repr.repr.clone(), id);
                    self.expr_into_place(
                        field,
                        PlaceRepr {
                            place: place,
                            repr: field_repr,
                        },
                    );
                }
            }
            &ir::AggregateKind::Variant(_, case_id, _) => {
                self.store_immediate(place_repr.place, case_id.into_u32() as i64);
                let (payload_place, payload_repr) =
                    self.project_downcast(place_repr.place, case_id, place_repr.repr);
                for (id, field) in fields.iter_enumerated() {
                    let (place, field_repr) =
                        self.project_field(payload_place.clone(), payload_repr.clone(), id);
                    self.expr_into_place(
                        field,
                        PlaceRepr {
                            place: place,
                            repr: field_repr,
                        },
                    );
                }
            }
        }
    }
    pub(super) fn expr_into_regs(&mut self, expr: &ir::Expr, dest: RegWindow, repr: Repr) {
        match &expr.kind {
            ir::ExprKind::Constant(constant) => {
                let value = self.eval_imm_constant(constant);
                self.load_immediate(dest.base, value);
            }
            ir::ExprKind::Load(place) => {
                let (src_place, _) = self.lower_place(place);
                self.codegen_copy(CodegenPlace::Reg(dest.base), src_place, repr.size_as_u16());
            }
            ir::ExprKind::Len(array) => {
                let (array, _) = self.lower_place(array);
                let (base, offset) = match array {
                    CodegenPlace::Reg(reg) => (reg, 0),
                    CodegenPlace::Offset(base, offset) => (base, offset),
                };
                self.push_instr(instructions::Instr::Load {
                    dst: dest.base,
                    src: instructions::Addr {
                        base,
                        offset: offset as u32 + 1,
                    },
                });
            }
            ir::ExprKind::Discriminant(place) => {
                self.codgen_discriminant(
                    PlaceRepr {
                        place: CodegenPlace::Reg(dest.base),
                        repr,
                    },
                    place,
                );
            }
            ir::ExprKind::Aggregate(aggregate_kind, fields) => {
                self.codegen_aggregrate(
                    PlaceRepr {
                        place: CodegenPlace::Reg(dest.base),
                        repr,
                    },
                    aggregate_kind,
                    fields,
                );
            }
            &ir::ExprKind::BinaryOp(op, ref left, ref right) => {
                if let Some(result) = self.simplify_binary_op(op, left, right) {
                    return self.expr_into_regs(&result, dest, repr);
                }
                if let BinaryOp::Add = op {
                    match (left.as_constant(), right.as_constant()) {
                        (Some(&ir::Constant::Int(value)), None) => {
                            let right = self.expr_as_regs(right, SCALAR_REPR).base;
                            self.add_imm(dest.base, right, value);
                            return;
                        }
                        (None, Some(&ir::Constant::Int(value))) => {
                            let left = self.expr_as_regs(left, SCALAR_REPR).base;
                            self.add_imm(dest.base, left, value);
                            return;
                        }
                        _ => (),
                    }
                }
                let left = self.expr_as_regs(left, SCALAR_REPR).base;
                let right = self.expr_as_regs(right, SCALAR_REPR).base;
                self.codegen_binary_op(dest, op, left, right);
            }
            ir::ExprKind::Not(expr) => {
                let dst = dest.base;
                let src = self.expr_as_regs(expr, SCALAR_REPR).base;
                self.push_instr(instructions::Instr::Not { dst, src });
            }
        }
    }
    fn codgen_discriminant(&mut self, place_repr: PlaceRepr, place: &ir::Place) {
        let (variant_place, variant_repr) = self.lower_place(place);
        let (tag_place, tag_repr) =
            self.project_field(variant_place, variant_repr, FieldId::new(0));
        self.codegen_copy(place_repr.place.into(), tag_place, tag_repr.size_as_u16());
    }
    pub(super) fn expr_into_place(&mut self, expr: &ir::Expr, place_repr: PlaceRepr) {
        match &expr.kind {
            ir::ExprKind::Load(src) => {
                let (src_place, repr) = self.lower_place(src);
                self.codegen_copy(place_repr.place.into(), src_place, repr.size_as_u16());
            }
            ir::ExprKind::Discriminant(place) => {
                self.codgen_discriminant(place_repr, place);
            }
            ir::ExprKind::Aggregate(aggregate_kind, fields) => {
                self.codegen_aggregrate(place_repr, aggregate_kind, fields)
            }
            _ => {
                let size = place_repr.repr.size_as_u16();
                let dest = match place_repr.place {
                    CodegenPlace::Reg(reg) => RegWindow { base: reg, size },
                    CodegenPlace::Offset(..) => RegWindow {
                        base: self.reserve_registers(size),
                        size,
                    },
                };
                self.expr_into_regs(expr, dest, place_repr.repr);
                if let CodegenPlace::Offset(..) = place_repr.place {
                    self.codegen_copy(place_repr.place, CodegenPlace::Reg(dest.base), dest.size);
                }
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
                let left = self.expr_as_regs(left, SCALAR_REPR).base;
                return Conditional::Not(left);
            }
            _ => Conditional::Reg({
                let reg = self.expr_as_regs(expr, SCALAR_REPR).base;
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
                let reg = self.expr_as_regs(expr, SCALAR_REPR).base;
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
                ir::AggregateKind::Tuple | ir::AggregateKind::Record(..) => {
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
                let reg = self.expr_as_regs(expr, repr);
                args.push(CallArg::Reg(reg));
            }
        }
    }
}
