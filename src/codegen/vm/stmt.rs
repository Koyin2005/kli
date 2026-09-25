use crate::{
    codegen::vm::{CodegenFunction, ScalarResult},
    ir,
    vm::instructions,
};

impl CodegenFunction<'_> {
    pub fn lower_stmt_full(&mut self, stmt: &ir::Stmt) {
        self.lower_stmt(stmt);
        self.release_registers();
    }
    pub fn lower_stmt(&mut self, stmt: &ir::Stmt) {
        match stmt {
            ir::Stmt::Return(value) => {
                let result = self.lower_expr_result(value, None);
                self.push_result(&result);
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
                let (place, _) = self.lower_place(return_place);
                let function = self.lower_expr_result(callee, None).into_single_scalar();
                for arg in args {
                    let arg_result = self.lower_expr_result(arg, None);
                    self.push_result(&arg_result);
                }
                match function {
                    ScalarResult::Func(func) => {
                        self.push_instr(instructions::Instr::Call(func));
                    }
                    ScalarResult::Reg(reg) => {
                        self.push_instr(instructions::Instr::CallIndirect(reg));
                    }
                    ScalarResult::Index(base, index) => {
                        let reg = self.eval_load_index(base, index);
                        self.push_instr(instructions::Instr::CallIndirect(reg));
                    }
                    ScalarResult::ConstIndex(base, index) => {
                        let reg = self.reserve_register();
                        self.load_index_imm(reg, base, index);
                        self.push_instr(instructions::Instr::CallIndirect(reg));
                    }
                    ScalarResult::Int(_) => unreachable!(),
                }
                self.pop_place(&place);
            }
            ir::Stmt::Print { value, is_err } => {
                let result = self.lower_expr_result(value, None);
                self.push_result(&result);
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
                let value = self.lower_expr_result(value, Some(&place));
                if value != self.load_place(&place, &repr) {
                    self.store_place(&place, &value);
                }
            }
            ir::Stmt::PanicIf(value) => {
                let (negated, value) = self.lower_cond_expr(value);
                let value = value.into_single_scalar();
                let ScalarResult::Int(value) = value else {
                    self.panic_if(&value, !negated);
                    return;
                };
                if value != 0 {
                    self.panic();
                    return;
                }
            }
            ir::Stmt::Loop(..) => todo!("loop"),
            ir::Stmt::Break(_) => todo!("break"),
            ir::Stmt::If(condition, then_branch, else_branch) => {
                let scalar = self.lower_expr_result(condition, None).into_single_scalar();
                if let ScalarResult::Int(n) = scalar {
                    let stmts = if n == 0 { else_branch } else { then_branch };
                    for stmt in stmts {
                        self.lower_stmt(stmt);
                    }
                    return;
                }
                if then_branch.is_empty() && else_branch.is_empty() {
                    return;
                }
                let cond_jump = {
                    let reg = self.force_scalar_in_reg(&scalar);
                    let cond_jump = self.push_instr_offset(instructions::Instr::JumpIfFalse(
                        reg,
                        instructions::JumpOffset(0),
                    ));
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
