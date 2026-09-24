use std::collections::HashMap;

use crate::{ir, vm::instructions};

type Instance = ir::BodyId;

enum ExprResult {
    Scalar(instructions::Reg),
    Tuple(Vec<ExprResult>),
}
struct CodegenFunction<'a> {
    id: ir::BodyId,
    function: instructions::FunctionId,
    result_function: instructions::Function,
    codgen: &'a mut Codegen,
    program: &'a ir::Program,
}
impl CodegenFunction<'_> {
    fn lower_expr_result(&mut self, expr: &ir::Expr) -> ExprResult {
        match &expr.kind {
            ir::ExprKind::Constant(_) => todo!("constants"),
            ir::ExprKind::Load(_) => todo!("load"),
            ir::ExprKind::Len(_) => todo!("len"),
            ir::ExprKind::Discriminant(_) => todo!("discriminant"),
            ir::ExprKind::Aggregate(kind, fields) => match kind {
                ir::AggregateKind::Tuple => ExprResult::Tuple(
                    fields
                        .iter()
                        .map(|field| self.lower_expr_result(field))
                        .collect(),
                ),
                ir::AggregateKind::Named => todo!("named"),
                ir::AggregateKind::Variant(..) => todo!("variant"),
            },
            ir::ExprKind::BinaryOp(..) => todo!("binary op"),
            ir::ExprKind::Not(_) => todo!("not"),
        }
    }
    fn push_instr(&mut self, instr: instructions::Instr) {
        self.result_function.instrs.push(instr);
    }
    fn push_result(&mut self, result: ExprResult) {
        match result {
            ExprResult::Scalar(value) => {
                self.push_instr(instructions::Instr::Push(value));
            }
            ExprResult::Tuple(elements) => {
                for element in elements {
                    self.push_result(element);
                }
            }
        }
    }
    fn lower(mut self) {
        for stmt in &self.program.bodies[self.id].body {
            match stmt {
                ir::Stmt::Return(value) => {
                    let result = self.lower_expr_result(value);
                    self.push_result(result);
                    self.push_instr(instructions::Instr::Return);
                }
                _ => todo!("{stmt:?}"),
            }
        }

        self.codgen.result.functions[self.function] = self.result_function;
    }
}

pub(super) struct Codegen {
    function_map: HashMap<Instance, instructions::FunctionId>,
    result: instructions::Program,
}
impl Codegen {
    pub fn new() -> Self {
        Self {
            result: instructions::Program::new(),
            function_map: HashMap::new(),
        }
    }
    fn push_function(&mut self, function: instructions::Function) -> instructions::FunctionId {
        self.result.functions.push(function)
    }
    fn function_for(
        &mut self,
        body_id: ir::BodyId,
        program: &ir::Program,
    ) -> instructions::FunctionId {
        if let Some(id) = self.function_map.get(&body_id) {
            return *id;
        }
        let id = self.push_function(instructions::Function {
            registers: 0,
            instrs: vec![],
        });
        self.function_map.insert(body_id, id);
        CodegenFunction {
            id: body_id,
            function: id,
            result_function: instructions::Function {
                registers: 0,
                instrs: Vec::new(),
            },
            codgen: self,
            program,
        }
        .lower();
        id
    }
    fn make_entrypoint_function(&mut self, program: &ir::Program) -> instructions::FunctionId {
        let instrs = if let Some(entrypoint) = program.entrypoint {
            let id = self.function_for(entrypoint, program);
            vec![instructions::Instr::Call(id), instructions::Instr::Return]
        } else {
            vec![instructions::Instr::Return]
        };
        self.push_function(instructions::Function {
            registers: 0,
            instrs: instrs,
        })
    }
    pub fn lower_program(
        mut self,
        program: &ir::Program,
    ) -> (instructions::Program, instructions::FunctionId) {
        let entrypoint = self.make_entrypoint_function(program);
        let program = self.result;
        (program, entrypoint)
    }
}

pub fn codegen(program: ir::Program) -> (instructions::Program, instructions::FunctionId) {
    let (program, entrypoint) = Codegen::new().lower_program(&program);
    (program, entrypoint)
}
