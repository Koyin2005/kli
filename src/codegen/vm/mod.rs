use std::collections::HashMap;

use crate::{index_vec::IndexVec, ir, typed_ast::FieldId, vm::instructions};

type Instance = ir::BodyId;

#[derive(Clone, Debug)]
enum Repr {
    Scalar,
    Tuple(IndexVec<FieldId, Repr>),
}

#[derive(Clone, Debug)]
enum LoweredPlace {
    Reg(instructions::Reg),
    Tuple(IndexVec<FieldId, LoweredPlace>),
}
enum ScalarResult {
    Reg(instructions::Reg),
    Func(instructions::FunctionId),
}
enum ExprResult {
    Scalar(ScalarResult),
    Tuple(Vec<ExprResult>),
}
struct CodegenFunction<'a> {
    id: ir::BodyId,
    function: instructions::FunctionId,
    result_function: instructions::Function,
    codgen: &'a mut Codegen,
    program: &'a ir::Program,
    locals: IndexVec<ir::Local, LoweredPlace>,
    local_reg_end: u16,
    next_reg: u16,
    max_reg: u16,
}
impl CodegenFunction<'_> {
    fn reserve_register(&mut self) -> instructions::Reg {
        let reg = self.next_reg;
        self.next_reg = self.next_reg.checked_add(1).expect("too many registers");
        self.max_reg = self.max_reg.max(self.next_reg);
        instructions::Reg::new(reg)
    }
    fn release_registers(&mut self) {
        self.next_reg = self.local_reg_end;
    }
    fn lower_expr_result(&mut self, expr: &ir::Expr) -> ExprResult {
        match &expr.kind {
            ir::ExprKind::Constant(constant) => match constant {
                ir::Constant::Int(_) => todo!("const int"),
                ir::Constant::Bool(_) => todo!("const bool"),
                ir::Constant::Function(id, args) => {
                    if !args.is_empty() {
                        todo!("handle generic functions")
                    }
                    let id = self.codgen.function_for(*id, self.program);
                    ExprResult::Scalar(ScalarResult::Func(id))
                }
                ir::Constant::String(value) => {
                    let reg = self.reserve_register();
                    value.with_str(|s| {
                        let s = self.codgen.string_index(s);
                        self.push_immediate(reg, s);
                    });
                    ExprResult::Scalar(ScalarResult::Reg(reg))
                }
                ir::Constant::Char(_) => todo!("char"),
            },
            ir::ExprKind::Load(place) => {
                let place = self.lower_place(place);
                self.load_place(place)
            }
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
    fn push_immediate(&mut self, reg: instructions::Reg, value: i64) {
        self.push_instr(instructions::Instr::LoadImmediate(reg, value));
    }
    fn push_intr_call(
        &mut self,
        instrinsic: instructions::Intrinsic,
        result: Option<LoweredPlace>,
    ) {
        self.result_function
            .instrs
            .push(instructions::Instr::CallIntrinisic(instrinsic));
        if let Some(result) = result {
            self.pop_place(result);
        }
    }
    fn push_result(&mut self, result: ExprResult) {
        match result {
            ExprResult::Scalar(value) => {
                match value {
                    ScalarResult::Reg(value) => self.push_instr(instructions::Instr::Push(value)),
                    ScalarResult::Func(_) => {
                        todo!("func instruction")
                    }
                };
            }
            ExprResult::Tuple(elements) => {
                for element in elements {
                    self.push_result(element);
                }
            }
        }
    }
    fn load_place(&mut self, place: LoweredPlace) -> ExprResult {
        match place {
            LoweredPlace::Reg(reg) => ExprResult::Scalar(ScalarResult::Reg(reg)),
            LoweredPlace::Tuple(fields) => ExprResult::Tuple(
                fields
                    .into_iter()
                    .map(|field| self.load_place(field))
                    .collect(),
            ),
        }
    }
    fn pop_place(&mut self, place: LoweredPlace) {
        match place {
            LoweredPlace::Reg(reg) => {
                self.push_instr(instructions::Instr::Pop(reg));
            }
            LoweredPlace::Tuple(fields) => {
                for field in fields.into_iter().rev() {
                    self.pop_place(field);
                }
            }
        }
    }
    fn lower_place(&mut self, place: &ir::Place) -> LoweredPlace {
        match place {
            ir::Place::Local(local) => self.locals[*local].clone(),
            ir::Place::Field(place, field_id) => {
                let LoweredPlace::Tuple(fields) = self.lower_place(place) else {
                    unreachable!("should be a tuple")
                };
                fields[*field_id].clone()
            }
            ir::Place::Deref(_) => todo!(),
            ir::Place::Downcast(..) => todo!(),
            ir::Place::Index(..) => todo!(),
        }
    }
    fn create_local_for(&mut self, repr: &Repr) -> LoweredPlace {
        match repr {
            Repr::Scalar => {
                let reg = self.reserve_register();
                LoweredPlace::Reg(reg)
            }
            Repr::Tuple(fields) => LoweredPlace::Tuple(
                fields
                    .iter()
                    .map(|field| self.create_local_for(field))
                    .collect(),
            ),
        }
    }
    fn lower_stmt(&mut self, stmt: &ir::Stmt) {
        match stmt {
            ir::Stmt::Return(value) => {
                let result = self.lower_expr_result(value);
                self.push_result(result);
                self.push_instr(instructions::Instr::Return);
            }
            ir::Stmt::Panic => {
                self.push_instr(instructions::Instr::CallIntrinisic(
                    instructions::Intrinsic::Panic,
                ));
            }
            ir::Stmt::Call(call) => {
                let ir::Call {
                    return_place,
                    callee,
                    args,
                } = call;
                let ExprResult::Scalar(function) = self.lower_expr_result(callee) else {
                    unreachable!("functions should always be scalar")
                };
                for arg in args {
                    let arg_result = self.lower_expr_result(arg);
                    self.push_result(arg_result);
                }
                match function {
                    ScalarResult::Func(func) => {
                        self.push_instr(instructions::Instr::Call(func));
                    }
                    ScalarResult::Reg(reg) => {
                        self.push_instr(instructions::Instr::CallIndirect(reg));
                    }
                }
                let place = self.lower_place(return_place);
                self.pop_place(place);
            }
            ir::Stmt::Print { value, is_err } => {
                let result @ ExprResult::Scalar(_) = self.lower_expr_result(value) else {
                    unreachable!("strings are always scalar")
                };
                self.push_result(result);
                self.push_intr_call(
                    if *is_err {
                        instructions::Intrinsic::Eprint
                    } else {
                        instructions::Intrinsic::Print
                    },
                    None,
                );
            }
            _ => todo!("{stmt:?}"),
        }
    }
    fn lower(mut self) {
        for local in &self.program.bodies[self.id].locals {
            let place = self.create_local_for(&self.codgen.type_repr(&local.ty));
            self.locals.push(place);
        }
        self.local_reg_end = self.next_reg;
        for stmt in &self.program.bodies[self.id].body {
            self.lower_stmt(stmt);
            self.release_registers();
        }
        self.result_function.registers = self.max_reg;
        self.codgen.result.functions[self.function] = self.result_function;
    }
}

pub(super) struct Codegen {
    function_map: HashMap<Instance, instructions::FunctionId>,
    string_map: HashMap<String, i64>,
    result: instructions::Program,
}
impl Codegen {
    pub fn new() -> Self {
        Self {
            result: instructions::Program::new(),
            string_map: HashMap::new(),
            function_map: HashMap::new(),
        }
    }
    fn type_repr(&self, ty: &ir::Type) -> Repr {
        match ty {
            ir::Type::Int => Repr::Scalar,
            ir::Type::Bool => Repr::Scalar,
            ir::Type::String => Repr::Scalar,
            ir::Type::Char => Repr::Scalar,
            ir::Type::Never => Repr::Tuple(IndexVec::new()),
            ir::Type::Param(_) => todo!(),
            ir::Type::Function(_, _) => Repr::Scalar,
            ir::Type::Tuple(fields) => {
                Repr::Tuple(fields.iter().map(|ty| self.type_repr(ty)).collect())
            }
            ir::Type::Array(_) => todo!(),
            ir::Type::Named(..) => todo!(),
            ir::Type::Box(_) => todo!(),
        }
    }
    fn string_index(&mut self, s: &str) -> i64 {
        if let Some(i) = self.string_map.get(s) {
            return *i;
        }
        let index = self
            .result
            .strings
            .len()
            .try_into()
            .expect("too many strings");
        let s = s.to_string();
        self.result.strings.push(s.clone());
        self.string_map.insert(s, index);
        index
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
            locals: IndexVec::new(),
            local_reg_end: 0,
            next_reg: 0,
            max_reg: 0,
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
    ) -> (
        instructions::Program,
        instructions::FunctionId,
        HashMap<instructions::FunctionId, ir::BodyId>,
    ) {
        let entrypoint = self.make_entrypoint_function(program);
        let program = self.result;
        let map = self
            .function_map
            .into_iter()
            .map(|(first, second)| (second, first))
            .collect();
        (program, entrypoint, map)
    }
}

pub fn codegen(ir_program: ir::Program) -> (instructions::Program, instructions::FunctionId) {
    let (program, entrypoint, map) = Codegen::new().lower_program(&ir_program);
    for (i, function) in program.functions.iter_enumerated() {
        if let Some(&id) = map.get(&i) {
            println!("body {}", ir_program.bodies[id].name);
        }
        println!("function {:?}", i.into_usize());
        println!("regs: {:?}", function.registers);
        for (i, instr) in function.instrs.iter().enumerate() {
            println!("{i:?} {:?}", instr)
        }
        println!();
    }
    (program, entrypoint)
}
