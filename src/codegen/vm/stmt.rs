use crate::{
    codegen::vm::{
        CodegenFunction, CodegenPlace, JumpIf, PlaceRepr, SCALAR_REPR,
        expr::{CallArg, Callee, Conditional},
    },
    ir,
    vm::instructions,
};

impl CodegenFunction<'_> {
    fn push_call_args(&mut self, args: impl IntoIterator<Item = CallArg>) {
        for arg in args {
            match arg {
                CallArg::Scalar(value) if let Ok(value) = value.try_into() => {
                    self.push_instr(instructions::Instr::PushImm(value));
                }
                CallArg::Scalar(value) => {
                    let value = self.add_const(value);
                    self.push_instr(instructions::Instr::PushConst(value));
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
                let result = self.expr_as_regs(value, repr);
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
                let (place, return_repr) = self.lower_place(return_place);
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
                    repr: return_repr,
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
                let (place, repr) = self.lower_place(place);
                self.expr_into_place(value, PlaceRepr { place, repr });
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
                let jump = match condition {
                    Conditional::Bool(value) => {
                        let stmts = if value { then_branch } else { else_branch };
                        for stmt in stmts {
                            self.lower_stmt(stmt);
                        }
                        return;
                    }
                    Conditional::Reg(reg) => JumpIf::Zero(reg),
                    Conditional::Not(reg) => JumpIf::NotZero(reg),
                };
                let cond_jump = {
                    let cond_jump = self.push_jump_if(jump);
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
                ir::Allocate::Array(ty, elements) => {
                    let (place, repr) = self.lower_place(place);
                    let dest = match place {
                        CodegenPlace::Reg(reg) => reg,
                        CodegenPlace::Offset(..) => {
                            let size = repr.size_as_u16();
                            self.reserve_registers(size)
                        }
                    };
                    let element_count: u32 = elements.len().try_into().expect("too many elements");
                    let elem_repr = self.codegen.type_repr(ty, self.program, &self.args);
                    let buf = self.reserve_register();
                    self.push_instr(instructions::Instr::Alloc {
                        dst: buf,
                        count: element_count * u32::from(elem_repr.size_as_u16()),
                    });
                    for (i, element) in elements.iter().enumerate() {
                        self.expr_into_place(
                            element,
                            PlaceRepr {
                                place: CodegenPlace::Offset(buf, i as u32),
                                repr: elem_repr.clone(),
                            },
                        );
                    }

                    self.push_instr(instructions::Instr::Alloc {
                        dst: dest,
                        count: 3,
                    });

                    self.push_instr(instructions::Instr::Store {
                        dst: instructions::Addr {
                            base: dest,
                            offset: 0,
                        },
                        src: buf,
                    });
                    let count = self.reserve_register();
                    self.load_immediate(count, element_count.into());
                    self.push_instr(instructions::Instr::Store {
                        dst: instructions::Addr {
                            base: dest,
                            offset: 1,
                        },
                        src: count,
                    });
                    self.push_instr(instructions::Instr::Store {
                        dst: instructions::Addr {
                            base: dest,
                            offset: 2,
                        },
                        src: count,
                    });
                    if let CodegenPlace::Offset(..) = place {
                        self.codegen_copy(place, CodegenPlace::Reg(dest), 1);
                    }
                }
            },
        }
    }
}
