use crate::{define_id, index_vec::IndexVec};
#[derive(Clone, Copy)]
pub struct Reg(u16);
impl Reg {
    pub fn into_u16(self) -> u16 {
        self.0
    }
}

define_id!(FunctionId);
#[derive(Clone, Copy)]
pub struct JumpOffset(u32);
impl JumpOffset {
    pub fn into_u32(self) -> u32 {
        self.0
    }
}
#[derive(Clone, Copy)]
pub enum Instr {
    Move { dst: Reg, src: Reg },
    LoadImmediate(Reg, i64),
    Add { dst: Reg, src1: Reg, src2: Reg },
    Push(Reg),
    Pop(Reg),
    Call(FunctionId),
    CallIntrinisic(Intrinsic),
    CallIndirect(Reg),
    JumpIf(Reg, JumpOffset),
    Jump(JumpOffset),
    Return,
}
#[derive(Clone, Copy)]
pub enum Intrinsic {
    AddWithOverflow,
}
pub struct Function {
    pub registers: u16,
    pub instrs: Vec<Instr>,
}
pub struct Program {
    pub functions: IndexVec<FunctionId, Function>,
}
impl Program {
    pub fn new() -> Self {
        Self {
            functions: IndexVec::new(),
        }
    }
}
