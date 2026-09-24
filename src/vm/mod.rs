use crate::{
    index_vec::IndexVec,
    vm::instructions::{Function, FunctionId, Instr, Intrinsic, Program, Reg},
};

pub(crate) mod instructions;
#[derive(Debug)]
pub enum RuntimeError {
    Panic,
}
pub(super) struct Frame {
    current_function: FunctionId,
    regs: Vec<i64>,
    ip: usize,
}
impl Frame {
    fn read_reg(&self, reg: Reg) -> i64 {
        self.regs[usize::from(reg.into_u16())]
    }
    fn store_reg(&mut self, reg: Reg, value: i64) {
        self.regs[usize::from(reg.into_u16())] = value;
    }
}
pub struct VM {
    functions: IndexVec<FunctionId, Function>,
    frames: Vec<Frame>,
    stack: Vec<i64>,
    current_frame: Frame,
}
impl VM {
    pub fn new(entry_point: FunctionId, program: Program) -> Self {
        let frame = Frame {
            current_function: entry_point,
            regs: vec![0; program.functions[entry_point].registers as _],
            ip: 0,
        };
        Self {
            functions: program.functions,
            frames: Vec::new(),
            stack: Vec::new(),
            current_frame: frame,
        }
    }
    fn next_instr(&mut self) -> Instr {
        let frame = &mut self.current_frame;
        let instr = self.functions[frame.current_function].instrs[frame.ip];
        frame.ip += 1;
        instr
    }
    fn call(&mut self, function_id: FunctionId) {
        let mut new_regs = vec![0; self.functions[function_id].registers as _];
        for (reg, value) in new_regs.iter_mut().zip(self.stack.drain(..)) {
            *reg = value;
        }
        let new_frame = Frame {
            current_function: function_id,
            regs: new_regs,
            ip: 0,
        };
        self.frames
            .push(std::mem::replace(&mut self.current_frame, new_frame));
    }
    pub fn run(mut self) -> Result<(), RuntimeError> {
        loop {
            match self.next_instr() {
                Instr::LoadImmediate(dst, value) => {
                    self.current_frame.store_reg(dst, value);
                }
                Instr::Move { dst, src } => {
                    let src = self.current_frame.read_reg(src);
                    self.current_frame.store_reg(dst, src);
                }
                Instr::Add { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, src1.wrapping_add(src2));
                }
                Instr::Push(reg) => {
                    self.stack.push(self.current_frame.read_reg(reg));
                }
                Instr::Pop(reg) => {
                    self.current_frame.store_reg(reg, self.stack.pop().unwrap());
                }
                Instr::Call(function_id) => {
                    self.call(function_id);
                }
                Instr::CallIntrinisic(intrinsic) => match intrinsic {
                    Intrinsic::AddWithOverflow => {
                        let second = self.stack.pop().unwrap();
                        let first = self.stack.pop().unwrap();
                        let (result, overflowed) = first.overflowing_add(second);
                        self.stack.extend([result, overflowed as i64]);
                    }
                },
                Instr::CallIndirect(reg) => {
                    let id = self.current_frame.read_reg(reg);
                    let function_id = FunctionId::new(id as usize);
                    self.call(function_id);
                }
                Instr::JumpIf(reg, jump_offset) => {
                    if self.current_frame.read_reg(reg) != 0 {
                        self.current_frame.ip = jump_offset.into_u32() as _;
                    }
                }
                Instr::Jump(jump_offset) => {
                    self.current_frame.ip = jump_offset.into_u32() as _;
                }
                Instr::Return => {
                    let Some(frame) = self.frames.pop() else {
                        return Ok(());
                    };
                    self.current_frame = frame;
                }
            }
        }
    }
}
