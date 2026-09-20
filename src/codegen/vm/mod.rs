use crate::vm::{self, instructions};

pub struct Codegen {
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
