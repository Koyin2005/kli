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
    return_ip: usize,
}
impl Frame {
    fn read_reg(&self, reg: Reg) -> i64 {
        self.regs[usize::from(reg.into_u16())]
    }
    fn store_reg(&mut self, reg: Reg, value: i64) {
        self.regs[usize::from(reg.into_u16())] = value;
    }
    fn move_reg(&mut self, dst: Reg, src: Reg) {
        self.regs[usize::from(dst.into_u16())] = self.regs[usize::from(src.into_u16())];
    }
}
pub struct VM {
    functions: IndexVec<FunctionId, Function>,
    frames: Vec<Frame>,
    stack: Vec<i64>,
    constant_ints: Vec<i64>,
    current_frame: Frame,
    strings: Vec<String>,
    memory: Vec<i64>,
}
impl VM {
    pub fn new(entry_point: FunctionId, program: Program) -> Self {
        let frame = Frame {
            current_function: entry_point,
            regs: vec![0; program.functions[entry_point].registers as _],
            return_ip: 0,
        };
        Self {
            constant_ints: program.ints,
            functions: program.functions,
            frames: Vec::new(),
            stack: Vec::new(),
            current_frame: frame,
            strings: program.strings,
            memory: vec![0],
        }
    }
    fn next_instr(&mut self, ip: usize) -> Instr {
        self.functions[self.current_frame.current_function].instrs[ip]
    }
    fn call(&mut self, function_id: FunctionId, ip: usize) {
        let mut new_regs = vec![0; self.functions[function_id].registers as _];
        for (reg, value) in new_regs.iter_mut().zip(self.stack.drain(..)) {
            *reg = value;
        }
        let new_frame = Frame {
            current_function: function_id,
            regs: new_regs,
            return_ip: ip,
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
        let mut ip = 0usize;
        loop {
            let instr = self.next_instr(ip);
            ip = ip.wrapping_add(1);
            match instr {
                Instr::LoadConst(dst, value) => {
                    let value = self.constant_ints[value.0 as usize];
                    self.current_frame.store_reg(dst, value);
                }
                Instr::LoadImmediate(dst, value) => {
                    self.current_frame.store_reg(dst, value.into());
                }
                Instr::Move { dst, src } => {
                    self.current_frame.move_reg(dst, src);
                }
                Instr::Add { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    let src2 = self.current_frame.read_reg(src2);
                    self.current_frame.store_reg(dst, src1.wrapping_add(src2));
                }
                Instr::AddImm { dst, src1, src2 } => {
                    let src1 = self.current_frame.read_reg(src1);
                    self.current_frame
                        .store_reg(dst, src1.wrapping_add(src2.into()));
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
                Instr::PushConst(value) => {
                    let value = self.constant_ints[value.0 as usize];
                    self.stack.push(value);
                }
                Instr::PushImm(value) => {
                    let value = value.into();
                    self.stack.push(value);
                }
                Instr::Alloc { dst, count } => {
                    let ptr = self.memory.len();
                    self.memory.extend(std::iter::repeat_n(0, count as usize));
                    self.current_frame.store_reg(dst, ptr as i64);
                }
                Instr::Store { dst, src } => {
                    let addr = self.current_frame.read_reg(dst.base) as usize + dst.offset as usize;
                    self.memory[addr] = self.current_frame.read_reg(src);
                }
                Instr::Load { dst, src } => {
                    let addr = self.current_frame.read_reg(src.base) as usize + src.offset as usize;
                    self.current_frame.store_reg(dst, self.memory[addr]);
                }
                Instr::Copy { dst, src, count } => {
                    let dst_ptr = self.current_frame.read_reg(dst.base) as usize;
                    let src_ptr = self.current_frame.read_reg(src.base) as usize;
                    for i in 0..count {
                        self.memory[dst_ptr..][(dst.offset + i) as usize] =
                            self.memory[src_ptr..][(src.offset + i) as usize];
                    }
                }
                Instr::ArrayOffset {
                    dst,
                    base,
                    index,
                    size,
                } => {
                    let addr = self.current_frame.read_reg(base)
                        + self.current_frame.read_reg(index) * i64::from(size);
                    self.current_frame.store_reg(dst, addr);
                }
                Instr::Push(reg) => {
                    self.stack.push(self.current_frame.read_reg(reg));
                }
                Instr::Pop(reg) => {
                    self.current_frame.store_reg(reg, self.stack.pop().unwrap());
                }
                Instr::Call(function_id) => {
                    self.call(function_id, ip);
                    ip = 0;
                }
                Instr::CallIntrinisic(intrinsic) => match intrinsic {
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
                    self.call(function_id, ip);
                    ip = 0;
                }
                Instr::JumpIfNotZero(reg, jump_offset) => {
                    if self.current_frame.read_reg(reg) != 0 {
                        ip = jump_offset.0 as _;
                        continue;
                    }
                }
                Instr::JumpIfZero(reg, jump_offset) => {
                    if self.current_frame.read_reg(reg) == 0 {
                        ip = jump_offset.0 as _;
                        continue;
                    }
                }
                Instr::Jump(jump_offset) => {
                    ip = jump_offset.0 as _;
                    continue;
                }
                Instr::Return => {
                    ip = self.current_frame.return_ip;
                    let Some(frame) = self.frames.pop() else {
                        return Ok(());
                    };
                    self.current_frame = frame;
                    continue;
                }
            }
        }
    }
}
