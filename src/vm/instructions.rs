use crate::{define_id, index_vec::IndexVec};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reg(u16);
impl Reg {
    pub fn new(value: u16) -> Self {
        Self(value)
    }
    pub fn into_u16(self) -> u16 {
        self.0
    }
}

define_id!(FunctionId);
impl FunctionId {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}
#[derive(Clone, Copy, Debug)]
pub struct JumpOffset(pub u32);
#[derive(Clone, Copy, Debug)]
pub enum Instr {
    Move { dst: Reg, src: Reg },
    LoadImmediate(Reg, i64),
    Add { dst: Reg, src1: Reg, src2: Reg },
    Sub { dst: Reg, src1: Reg, src2: Reg },
    LesserThan { dst: Reg, src1: Reg, src2: Reg },
    GreaterThan { dst: Reg, src1: Reg, src2: Reg },
    Equals { dst: Reg, src1: Reg, src2: Reg },
    Not { dst: Reg, src: Reg },
    Push(Reg),
    PushImmediate(i64),
    Pop(Reg),
    Call(FunctionId),
    CallIntrinisic(Intrinsic),
    CallIndirect(Reg),
    JumpIfFalse(Reg, JumpOffset),
    JumpIf(Reg, JumpOffset),
    Jump(JumpOffset),
    Return,
}
#[derive(Clone, Copy, Debug)]
pub enum Intrinsic {
    AddWithOverflow,
    SubWithOverflow,
    Panic,
    Print,
    Eprint,
}
#[derive(Debug)]
pub struct Function {
    pub registers: u16,
    pub instrs: Vec<Instr>,
}
#[derive(Debug)]
pub struct Program {
    pub strings: Vec<String>,
    pub functions: IndexVec<FunctionId, Function>,
}
impl Program {
    pub fn new() -> Self {
        Self {
            functions: IndexVec::new(),
            strings: Vec::new(),
        }
    }
}
