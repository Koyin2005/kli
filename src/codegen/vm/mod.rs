use std::collections::HashMap;

use crate::{ir, vm::instructions};

type Instance = ir::BodyId;
pub(super) struct Codegen {
    function_map: HashMap<Instance, instructions::FunctionId>,
    result: instructions::Program,
    program: ir::Program,
}
impl Codegen {
    pub fn new(program: ir::Program) -> Self {
        Self {
            result: instructions::Program::new(),
            program,
            function_map: HashMap::new(),
        }
    }
    fn push_function(&mut self, function: instructions::Function) -> instructions::FunctionId {
        self.result.functions.push(function)
    }

    fn function_for(&mut self, body_id: ir::BodyId) -> instructions::FunctionId {
        if let Some(id) = self.function_map.get(&body_id) {
            return *id;
        }
        let id = self.push_function(instructions::Function {
            registers: 0,
            instrs: vec![],
        });
        self.function_map.insert(body_id, id);
        id
    }
    fn make_entrypoint_function(&mut self) -> instructions::FunctionId {
        let instrs = if let Some(entrypoint) = self.program.entrypoint {
            let id = self.function_for(entrypoint);
            vec![instructions::Instr::Call(id), instructions::Instr::Return]
        } else {
            vec![instructions::Instr::Return]
        };
        self.push_function(instructions::Function {
            registers: 0,
            instrs: instrs,
        })
    }
    pub fn lower_program(mut self) -> (instructions::Program, instructions::FunctionId) {
        let entrypoint = self.make_entrypoint_function();
        let program = self.result;
        (program, entrypoint)
    }
}

pub fn codegen(program: ir::Program) -> (instructions::Program, instructions::FunctionId) {
    let (program, entrypoint) = Codegen::new(program).lower_program();
    (program, entrypoint)
}
