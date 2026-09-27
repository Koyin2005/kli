use crate::{
    index_vec::IndexVec,
    vm::instructions::{Function, FunctionId, Instr, Intrinsic, Program, Reg},
};

pub(crate) mod instructions;
#[derive(Debug)]
pub enum RuntimeError {
    Panic,
    DivideByZero,
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
    strings: Vec<String>,
    arrays: Vec<Vec<i64>>,
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
            strings: program.strings,
            arrays: Vec::new(),
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
    fn print_string(&mut self, index: i64, err: bool) {
        let index: usize = index.try_into().expect("should be a usize");
        if err {
            eprint!("{}", self.strings[index]);
        } else {
            print!("{}", self.strings[index]);
        }
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
                Instr::AddImm { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    self.current_frame.store_reg(dst, src1.wrapping_add(src2));
                }
                Instr::Sub { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, src1.wrapping_sub(src2));
                }
                Instr::Mul { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, src1.wrapping_mul(src2));
                }
                Instr::Div { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    if src2 == 0 {
                        return Err(RuntimeError::DivideByZero);
                    }
                    self.current_frame.store_reg(dst, src1.wrapping_div(src2));
                }
                Instr::And { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, src1 & src2);
                }
                Instr::Or { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, src1 | src2);
                }
                Instr::LesserThan { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, (src1 < src2).into());
                }
                Instr::LesserThanUnsigned { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1).cast_unsigned();
                    let src2 = self.current_frame.read_reg(src2).cast_unsigned();
                    self.current_frame.store_reg(dst, (src1 < src2).into());
                }
                Instr::GreaterThan { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, (src1 > src2).into());
                }
                Instr::Equals { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, (src1 == src2).into());
                }
                Instr::Not { dst, src } => {
                    let src1 = self.current_frame.read_reg(src);
                    self.current_frame.store_reg(dst, (src1 == 0).into());
                }
                Instr::PushImmediate(value) => {
                    self.stack.push(value);
                }
                Instr::LoadIndex { dst, base, offset } => {
                    let base = self.current_frame.read_reg(base) as usize;
                    let offset = self.current_frame.read_reg(offset) as usize;
                    self.current_frame.store_reg(dst, self.arrays[base][offset]);
                }
                Instr::LoadIndexImm { dst, src, offset } => {
                    let base = self.current_frame.read_reg(src) as usize;
                    let offset = offset as usize;
                    self.current_frame.store_reg(dst, self.arrays[base][offset]);
                }
                Instr::StoreIndex { base, offset, src } => {
                    let base = self.current_frame.read_reg(base) as usize;
                    let offset = self.current_frame.read_reg(offset) as usize;
                    self.arrays[base][offset] = self.current_frame.read_reg(src);
                }
                Instr::StoreIndexImm { base, offset, src } => {
                    let base = self.current_frame.read_reg(base) as usize;
                    let offset = offset as usize;
                    self.arrays[base][offset] = self.current_frame.read_reg(src);
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
                    Intrinsic::Alloc => {
                        let index = self.arrays.len();
                        self.arrays.push(std::mem::take(&mut self.stack));
                        self.stack.push(index as _);
                    }
                    Intrinsic::AddWithOverflow => {
                        let second = self.stack.pop().unwrap();
                        let first = self.stack.pop().unwrap();
                        let (result, overflowed) = first.overflowing_add(second);
                        self.stack.extend([result, overflowed as i64]);
                    }
                    Intrinsic::SubWithOverflow => {
                        let second = self.stack.pop().unwrap();
                        let first = self.stack.pop().unwrap();
                        let (result, overflowed) = first.overflowing_sub(second);
                        self.stack.extend([result, overflowed as i64]);
                    }
                    Intrinsic::MulWithOverflow => {
                        let second = self.stack.pop().unwrap();
                        let first = self.stack.pop().unwrap();
                        let (result, overflowed) = first.overflowing_mul(second);
                        self.stack.extend([result, overflowed as i64]);
                    }
                    Intrinsic::Panic => {
                        return Err(RuntimeError::Panic);
                    }
                    Intrinsic::Print => {
                        let value = self.stack.pop().unwrap();
                        self.print_string(value, false);
                    }
                    Intrinsic::Eprint => {
                        let value = self.stack.pop().unwrap();
                        self.print_string(value, true);
                    }
                },
                Instr::CallIndirect(reg) => {
                    let id = self.current_frame.read_reg(reg);
                    let function_id = FunctionId::new(id as usize);
                    self.call(function_id);
                }
                Instr::JumpIfNotZero(reg, jump_offset) => {
                    if self.current_frame.read_reg(reg) != 0 {
                        self.current_frame.ip = jump_offset.0 as _;
                    }
                }
                Instr::JumpIfZero(reg, jump_offset) => {
                    if self.current_frame.read_reg(reg) == 0 {
                        self.current_frame.ip = jump_offset.0 as _;
                    }
                }
                Instr::Jump(jump_offset) => {
                    self.current_frame.ip = jump_offset.0 as _;
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
