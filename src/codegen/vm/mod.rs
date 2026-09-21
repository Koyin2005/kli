use crate::{
    ir,
    vm::instructions,
};

pub(super) struct Codegen {
    program: instructions::Program,
}
impl Codegen {
    pub fn new() -> Self {
        Self {
            program: instructions::Program::new(),
        }
    }
    pub fn finish(self) -> instructions::Program {
        self.program
    }
}

pub fn codegen(_program: ir::Program) -> instructions::Program {
    Codegen::new().finish()
}
