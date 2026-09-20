use crate::vm::{self, instructions};

pub struct Codegen {
    functions: instructions::Program,
}
impl Codegen {
    pub fn new() -> Self {
        Self {
            functions: instructions::Program::new(),
        }
    }

    pub fn finish(self) -> instructions::Program {
        self.functions
    }
}
