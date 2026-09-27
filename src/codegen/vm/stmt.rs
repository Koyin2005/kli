use crate::{
    codegen::vm::{
        CodegenFunction, JumpIf, PlaceRepr, SCALAR_REPR, ScalarResult,
        expr::{CallArg, Callee, Conditional},
    },
    ir,
    vm::instructions,
};

impl CodegenFunction<'_> {
    fn push_call_args(&mut self, args: impl IntoIterator<Item = CallArg>) {
        for arg in args {
            match arg {
                CallArg::Scalar(value) => {
                    self.push_instr(instructions::Instr::PushImmediate(value));
                }
                CallArg::Reg(regs) => {
                    for reg in regs.into_iter() {
                        self.push_reg_to_stack(reg);
                    }
                }
            }
        }
    }
    pub fn lower_stmt_full(&mut self, stmt: &ir::Stmt) {
        self.lower_stmt(stmt);
        self.release_registers();
    }
    pub fn lower_stmt(&mut self, stmt: &ir::Stmt) {
        match stmt {
            ir::Stmt::Return(value) => {
                let repr = self.codegen.type_repr(
                    &self.program.bodies[self.id].return_ty,
                    self.program,
                    &self.args,
                );
                let result = self.codegen_expr_into_reg_window(value, repr);
                for reg in result.into_iter() {
                    self.push_reg_to_stack(reg);
                }
                self.push_instr(instructions::Instr::Return);
            }
            ir::Stmt::Panic => {
                self.panic();
            }
            ir::Stmt::Call(call) => {
                let ir::Call {
                    return_place,
                    callee,
                    args,
                } = call;
                let (place, retrun_repr) = self.lower_codegen_place(return_place);
                let function = self.lower_callee(callee);
                {
                    let mut call_args = Vec::new();
                    for arg in args {
                        let ty = arg.type_of(self.program, &self.program.bodies[self.id]);
                        self.lower_call_arg(
                            arg,
                            self.codegen.type_repr(&ty, self.program, &self.args),
                            &mut call_args,
                        );
                        self.push_call_args(call_args.drain(..));
                    }
                }
                match function {
                    Callee::Function(func) => {
                        self.push_instr(instructions::Instr::Call(func));
                    }
                    Callee::Reg(reg) => {
                        self.push_instr(instructions::Instr::CallIndirect(reg));
                    }
                }
                self.pop_place(PlaceRepr {
                    place,
                    repr: retrun_repr,
                });
            }
            ir::Stmt::Print { value, is_err } => {
                {
                    let mut args = Vec::new();
                    self.lower_call_arg(value, SCALAR_REPR, &mut args);
                    self.push_call_args(args);
                }
                self.push_intr_call(
                    if *is_err {
                        instructions::Intrinsic::Eprint
                    } else {
                        instructions::Intrinsic::Print
                    },
                    None,
                );
            }
            ir::Stmt::Assign(place, value) => {
                let (place, repr) = self.lower_codegen_place(place);
                self.codegen_expr_into(value, PlaceRepr { place, repr });
            }
            ir::Stmt::PanicIf(value) => {
                let condition = self.lower_condition(value);
                self.codegen_panic_if(condition);
            }
            ir::Stmt::Loop(label, stmts) => {
                let start = self.current_jump_offset();
                self.loop_labels.insert(*label, (start, Vec::new()));
                for stmt in stmts {
                    self.lower_stmt_full(stmt);
                }
                self.push_instr(instructions::Instr::Jump(start));
                let (_, jumps) = self
                    .loop_labels
                    .remove(label)
                    .expect("should have info for this label");
                for jump in jumps {
                    self.patch_jump_current(jump);
                }
            }
            ir::Stmt::Break(label) => {
                let index =
                    self.push_instr_offset(instructions::Instr::Jump(instructions::JumpOffset(0)));
                let (_, jumps) = self
                    .loop_labels
                    .get_mut(label)
                    .expect("should have info for this label");
                jumps.push(index);
            }
            ir::Stmt::If(condition, then_branch, else_branch) => {
                let condition = self.lower_condition(condition);
                if then_branch.is_empty() && else_branch.is_empty() {
                    return;
                }
                let (reg, jump) = match condition {
                    Conditional::Bool(value) => {
                        let stmts = if value { else_branch } else { then_branch };
                        for stmt in stmts {
                            self.lower_stmt(stmt);
                        }
                        return;
                    }
                    Conditional::Reg(reg) => (reg, JumpIf::NotZero),
                    Conditional::Not(reg) => (reg, JumpIf::Zero),
                };
                let cond_jump = {
                    let cond_jump = self.push_jump_if(jump, reg);
                    self.release_registers();
                    cond_jump
                };
                for stmt in then_branch {
                    self.lower_stmt_full(stmt);
                }
                let end_jump =
                    self.push_instr_offset(instructions::Instr::Jump(instructions::JumpOffset(0)));
                self.patch_jump_current(cond_jump);
                for stmt in else_branch {
                    self.lower_stmt_full(stmt);
                }
                self.patch_jump_current(end_jump);
            }
            ir::Stmt::Match(_) => todo!("match"),
            ir::Stmt::ReadLine(_) => todo!("read_line"),
            ir::Stmt::Alloc(place, alloc) => match alloc {
                ir::Allocate::Array(_, elements) => {
                    let (place, _) = self.lower_place(place);
                    let element_count: i64 = elements.len().try_into().expect("too many elements");
                    for element in elements {
                        let result = self.lower_expr_result(element, None);
                        self.push_result(&result);
                    }
                    self.push_intr_call(instructions::Intrinsic::Alloc, None);
                    self.push_scalar_on_stack(&ScalarResult::Int(element_count));
                    self.push_scalar_on_stack(&ScalarResult::Int(element_count));
                    self.push_intr_call(instructions::Intrinsic::Alloc, Some(&place));
                }
            },
        }
    }
}
