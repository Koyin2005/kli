pub struct Reg(u16);
pub struct FunctionId(u32);
pub struct JumpOffset(i32);
pub enum Instr {
    Add{
        dst : Reg,
        src1 : Reg,
        src2 : Reg,
    },
    Push(Reg),
    Pop(Reg),
    Call(FunctionId),
    CallIndirect(Reg),
    JumpIf(Reg,JumpOffset),
    Jump(JumpOffset),
    Return
}