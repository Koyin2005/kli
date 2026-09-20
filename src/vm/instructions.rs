use crate::{define_id, index_vec::IndexVec};

pub struct Reg(u16);
define_id!(FunctionId);
pub struct JumpOffset(i32);
pub enum Instr {
    Add { dst: Reg, src1: Reg, src2: Reg },
    Push(Reg),
    Pop(Reg),
    Call(FunctionId),
    CallIndirect(Reg),
    JumpIf(Reg, JumpOffset),
    Jump(JumpOffset),
    Return,
}
pub struct Function {
    pub instrs: Vec<Instr>,
}
pub type Program = IndexVec<FunctionId,Function>;
