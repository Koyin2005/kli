use std::collections::HashMap;

use crate::{
    codegen::classify_locals, index_vec::IndexVec, ir, typed_ast::FieldId, vm::instructions,
};

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
#[derive(PartialEq, Eq)]
enum ScalarResult {
    Reg(instructions::Reg),
    Func(instructions::FunctionId),
    Int(i64),
}
impl ScalarResult {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ScalarResult::Reg(_) => None,
            ScalarResult::Func(function_id) => Some(function_id.as_u32().into()),
            ScalarResult::Int(value) => Some(*value),
        }
    }
}
impl From<ScalarResult> for ExprResult {
    fn from(value: ScalarResult) -> Self {
        Self::Scalar(value)
    }
}
impl From<bool> for ExprResult {
    fn from(value: bool) -> Self {
        Self::Scalar(ScalarResult::Int(value.into()))
    }
}
impl From<i64> for ScalarResult {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
impl From<i64> for ExprResult {
    fn from(value: i64) -> Self {
        Self::Scalar(value.into())
    }
}
impl From<instructions::Reg> for ExprResult {
    fn from(value: instructions::Reg) -> Self {
        Self::Scalar(ScalarResult::Reg(value))
    }
}
impl From<instructions::Reg> for ScalarResult {
    fn from(value: instructions::Reg) -> Self {
        ScalarResult::Reg(value)
    }
}
enum BinaryOpInstr {
    Add,
    Sub,
    Div,
    Mul,
    Lt,
    Gt,
    Eq,
}
#[derive(PartialEq, Eq)]
enum ExprResult {
    Scalar(ScalarResult),
    Tuple(Vec<ExprResult>),
}
impl ExprResult {
    fn pair(first: impl Into<Self>, second: impl Into<Self>) -> Self {
        Self::Tuple(vec![first.into(), second.into()])
    }
}
struct CodegenFunction<'a> {
    id: ir::BodyId,
    function: instructions::FunctionId,
    result_function: instructions::Function,
    codegen: &'a mut Codegen,
    program: &'a ir::Program,
    locals: IndexVec<ir::Local, LoweredPlace>,
    local_reg_end: u16,
    next_reg: u16,
    max_reg: u16,
    panic_jumps: Vec<usize>,
    _local_info: IndexVec<ir::Local, super::AssignCount>,
}
impl<'a> CodegenFunction<'a> {
    fn new(
        id: ir::BodyId,
        function: instructions::FunctionId,
        codegen: &'a mut Codegen,
        program: &'a ir::Program,
    ) -> Self {
        let body = &program.bodies[id];
        Self {
            _local_info: classify_locals(body),
            id,
            function,
            result_function: instructions::Function {
                registers: 0,
                instrs: Vec::new(),
            },
            codegen,
            program,
            locals: IndexVec::new(),
            local_reg_end: 0,
            next_reg: 0,
            max_reg: 0,
            panic_jumps: Vec::new(),
        }
    }
    fn reserve_register(&mut self) -> instructions::Reg {
        let reg = self.next_reg;
        self.next_reg = self.next_reg.checked_add(1).expect("too many registers");
        self.max_reg = self.max_reg.max(self.next_reg);
        instructions::Reg::new(reg)
    }
    fn release_registers(&mut self) {
        self.next_reg = self.local_reg_end;
    }
    fn eval_overflow_op(
        &mut self,
        result_place: Option<&LoweredPlace>,
        op: ir::OverflowOp,
        left: &ScalarResult,
        right: &ScalarResult,
    ) -> ExprResult {
        let instrinsic = match op {
            ir::OverflowOp::Add => instructions::Intrinsic::AddWithOverflow,
            ir::OverflowOp::Sub => instructions::Intrinsic::SubWithOverflow,
        };
        self.push_scalar_on_stack(&left);
        self.push_scalar_on_stack(&right);
        if let Some(result_place) = result_place {
            self.push_intr_call(instrinsic, Some(result_place));
            return self.load_place(&result_place);
        }
        let left_reg = self.reserve_register();
        let right_reg = self.reserve_register();
        self.push_intr_call(
            instrinsic,
            Some(&LoweredPlace::Tuple(IndexVec::from([
                LoweredPlace::Reg(left_reg),
                LoweredPlace::Reg(right_reg),
            ]))),
        );
        ExprResult::pair(left_reg, right_reg)
    }
    fn eval_binary_op(
        &mut self,
        op: BinaryOpInstr,
        result_place: Option<&LoweredPlace>,
        left: &ScalarResult,
        right: &ScalarResult,
    ) -> ExprResult {
        let dst = if let Some(result) = result_place {
            let LoweredPlace::Reg(reg) = result else {
                unreachable!("should be a scalar");
            };
            *reg
        } else {
            self.reserve_register()
        };
        let src1 = self.force_scalar_in_reg(&left);
        let src2 = self.force_scalar_in_reg(&right);
        let instr = match op {
            BinaryOpInstr::Add => instructions::Instr::Add { dst, src1, src2 },
            BinaryOpInstr::Sub => instructions::Instr::Sub { dst, src1, src2 },
            BinaryOpInstr::Div => todo!(),
            BinaryOpInstr::Mul => todo!(),
            BinaryOpInstr::Lt => instructions::Instr::LesserThan { dst, src1, src2 },
            BinaryOpInstr::Gt => instructions::Instr::GreaterThan { dst, src1, src2 },
            BinaryOpInstr::Eq => instructions::Instr::Equals { dst, src1, src2 },
        };
        self.push_instr(instr);
        ExprResult::Scalar(ScalarResult::Reg(dst))
    }
    fn lower_binary_op(
        &mut self,
        op: ir::BinaryOp,
        left: ScalarResult,
        right: ScalarResult,
        result_place: Option<&LoweredPlace>,
    ) -> ExprResult {
        match op {
            ir::BinaryOp::Add => {
                self.eval_binary_op(BinaryOpInstr::Add, result_place, &left, &right)
            }
            ir::BinaryOp::AddWithOverflow => {
                self.eval_overflow_op(result_place, ir::OverflowOp::Add, &left, &right)
            }
            ir::BinaryOp::Subtract => {
                self.eval_binary_op(BinaryOpInstr::Sub, result_place, &left, &right)
            }
            ir::BinaryOp::SubtractWithOverflow => {
                self.eval_overflow_op(result_place, ir::OverflowOp::Sub, &left, &right)
            }
            ir::BinaryOp::Lesser => {
                self.eval_binary_op(BinaryOpInstr::Lt, result_place, &left, &right)
            }
            ir::BinaryOp::Greater => {
                self.eval_binary_op(BinaryOpInstr::Gt, result_place, &left, &right)
            }
            ir::BinaryOp::Equals => {
                self.eval_binary_op(BinaryOpInstr::Eq, result_place, &left, &right)
            }
            ir::BinaryOp::InBounds => todo!(),
        }
    }
    fn lower_expr_result(&mut self, expr: &ir::Expr, result: Option<&LoweredPlace>) -> ExprResult {
        match &expr.kind {
            ir::ExprKind::Constant(constant) => match constant {
                ir::Constant::Int(value) => ExprResult::Scalar(ScalarResult::Int(*value)),
                ir::Constant::Bool(value) => ExprResult::Scalar(ScalarResult::Int((*value).into())),
                ir::Constant::Function(id, args) => {
                    if !args.is_empty() {
                        todo!("handle generic functions")
                    }
                    let id = self.codegen.function_for(*id, self.program);
                    ExprResult::Scalar(ScalarResult::Func(id))
                }
                ir::Constant::String(value) => {
                    let index = value.with_str(|s| self.codegen.string_index(s));
                    ExprResult::Scalar(ScalarResult::Int(index))
                }
                &ir::Constant::Char(value) => {
                    ExprResult::Scalar(ScalarResult::Int(u32::from(value).into()))
                }
            },
            ir::ExprKind::Load(place) => {
                let place = self.lower_place(place);
                self.load_place(&place)
            }
            ir::ExprKind::Len(_) => todo!("len"),
            ir::ExprKind::Discriminant(_) => todo!("discriminant"),
            ir::ExprKind::Aggregate(kind, fields) => match kind {
                ir::AggregateKind::Tuple => ExprResult::Tuple(
                    fields
                        .iter()
                        .map(|field| self.lower_expr_result(field, None))
                        .collect(),
                ),
                ir::AggregateKind::Named => todo!("named"),
                ir::AggregateKind::Variant(..) => todo!("variant"),
            },
            ir::ExprKind::BinaryOp(op, left, right) => {
                let ExprResult::Scalar(left) = self.lower_expr_result(left, None) else {
                    unreachable!("should be a scalar")
                };
                let ExprResult::Scalar(right) = self.lower_expr_result(right, None) else {
                    unreachable!("should be a scalar")
                };
                self.lower_binary_op(*op, left, right, result)
            }
            ir::ExprKind::Not(value) => {
                let ExprResult::Scalar(value) = self.lower_expr_result(value, None) else {
                    unreachable!("should be a scalar")
                };
                if let ScalarResult::Int(value) = value {
                    return ExprResult::Scalar(ScalarResult::Int((value == 0).into()));
                }
                let reg = self.force_scalar_in_reg(&value);
                let dst_reg = if let Some(place) = result {
                    let LoweredPlace::Reg(reg) = place else {
                        unreachable!("should be a scalar")
                    };
                    *reg
                } else {
                    self.reserve_register()
                };
                self.push_instr(instructions::Instr::Not {
                    dst: dst_reg,
                    src: reg,
                });
                ExprResult::Scalar(ScalarResult::Reg(reg))
            }
        }
    }
    fn push_instr(&mut self, instr: instructions::Instr) {
        self.result_function.instrs.push(instr);
    }
    fn push_instr_offset(&mut self, instr: instructions::Instr) -> usize {
        let offset = self.result_function.instrs.len();
        self.push_instr(instr);
        offset
    }
    fn push_immediate(&mut self, reg: instructions::Reg, value: i64) {
        self.push_instr(instructions::Instr::LoadImmediate(reg, value));
    }
    fn push_move(&mut self, dst: instructions::Reg, src: instructions::Reg) {
        self.push_instr(instructions::Instr::Move { dst, src });
    }
    fn push_intr_call(
        &mut self,
        instrinsic: instructions::Intrinsic,
        result: Option<&LoweredPlace>,
    ) {
        self.result_function
            .instrs
            .push(instructions::Instr::CallIntrinisic(instrinsic));
        if let Some(result) = result {
            self.pop_place(result);
        }
    }
    fn push_scalar_on_stack(&mut self, value: &ScalarResult) {
        let reg = match value.as_i64() {
            Some(value) => {
                self.push_instr(instructions::Instr::PushImmediate(value));
                return;
            }
            None => self.force_scalar_in_reg(value),
        };
        self.push_instr(instructions::Instr::Push(reg));
    }
    fn push_result(&mut self, result: &ExprResult) {
        match result {
            ExprResult::Scalar(value) => {
                self.push_scalar_on_stack(value);
            }
            ExprResult::Tuple(elements) => {
                for element in elements {
                    self.push_result(element);
                }
            }
        }
    }
    fn load_place(&self, place: &LoweredPlace) -> ExprResult {
        match place {
            &LoweredPlace::Reg(reg) => ExprResult::Scalar(ScalarResult::Reg(reg)),
            LoweredPlace::Tuple(fields) => ExprResult::Tuple(
                fields
                    .into_iter()
                    .map(|field| self.load_place(field))
                    .collect(),
            ),
        }
    }
    fn pop_place(&mut self, place: &LoweredPlace) {
        match place {
            &LoweredPlace::Reg(reg) => {
                self.push_instr(instructions::Instr::Pop(reg));
            }
            LoweredPlace::Tuple(fields) => {
                for field in fields.into_iter().rev() {
                    self.pop_place(field);
                }
            }
        }
    }
    fn force_scalar_in_reg(&mut self, value: &ScalarResult) -> instructions::Reg {
        match value {
            ScalarResult::Reg(reg) => *reg,
            _ => {
                let reg = self.reserve_register();
                self.store_scalar_in_reg(reg, value);
                reg
            }
        }
    }
    fn store_scalar_in_reg(&mut self, reg: instructions::Reg, value: &ScalarResult) {
        match value {
            &ScalarResult::Func(func) => {
                self.push_immediate(
                    reg,
                    func.into_usize().try_into().expect("too many functions"),
                );
            }
            &ScalarResult::Int(value) => {
                self.push_immediate(reg, value);
            }
            &ScalarResult::Reg(src) => {
                self.push_move(reg, src);
            }
        }
    }
    #[track_caller]
    fn store_place(&mut self, place: LoweredPlace, result: &ExprResult) {
        match (place, result) {
            (LoweredPlace::Reg(reg), ExprResult::Scalar(value)) => {
                self.store_scalar_in_reg(reg, value)
            }
            (LoweredPlace::Tuple(places), ExprResult::Tuple(results)) => {
                for (place, result) in places.into_iter().zip(results) {
                    self.store_place(place, result);
                }
            }
            (LoweredPlace::Reg(_) | LoweredPlace::Tuple(_), _) => {
                panic!("invalid place to result store")
            }
        }
    }
    fn lower_place(&self, place: &ir::Place) -> LoweredPlace {
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
    fn current_jump_offset(&self) -> instructions::JumpOffset {
        instructions::JumpOffset(
            self.result_function
                .instrs
                .len()
                .try_into()
                .expect("too many instructions"),
        )
    }
    fn patch_jump_current(&mut self, instr: usize) {
        let new_offset = self.current_jump_offset();
        self.patch_jump(instr, new_offset);
    }
    fn patch_jump(&mut self, instr_index: usize, new_offset: instructions::JumpOffset) {
        let instr = &mut self.result_function.instrs[instr_index];
        let (instructions::Instr::Jump(offset)
        | instructions::Instr::JumpIf(_, offset)
        | instructions::Instr::JumpIfFalse(_, offset)) = instr
        else {
            panic!("cannot patch non jump instruction {instr:?} at {instr_index}")
        };
        *offset = new_offset;
    }
    fn panic_if(&mut self, result: &ScalarResult) {
        let reg = self.force_scalar_in_reg(result);
        let index = self.push_instr_offset(instructions::Instr::JumpIf(
            reg,
            instructions::JumpOffset(0),
        ));
        self.panic_jumps.push(index);
    }
    fn panic(&mut self) {
        let index = self.push_instr_offset(instructions::Instr::Jump(instructions::JumpOffset(0)));
        self.panic_jumps.push(index);
    }
    fn lower_stmt_full(&mut self, stmt: &ir::Stmt) {
        self.lower_stmt(stmt);
        self.release_registers();
    }
    fn lower_stmt(&mut self, stmt: &ir::Stmt) {
        match stmt {
            ir::Stmt::Return(value) => {
                let result = self.lower_expr_result(value, None);
                self.push_result(&result);
                self.push_instr(instructions::Instr::Return);
            }
            ir::Stmt::Panic => {
                self.panic();
            }
            ir::Stmt::Call(call) => {
                let ir::Call {
                    return_place,
                    callee,
                    args,
                } = call;
                let place = self.lower_place(return_place);
                let ExprResult::Scalar(function) = self.lower_expr_result(callee, None) else {
                    unreachable!("functions should always be scalar")
                };
                for arg in args {
                    let arg_result = self.lower_expr_result(arg, None);
                    self.push_result(&arg_result);
                }
                match function {
                    ScalarResult::Func(func) => {
                        self.push_instr(instructions::Instr::Call(func));
                    }
                    ScalarResult::Reg(reg) => {
                        self.push_instr(instructions::Instr::CallIndirect(reg));
                    }
                    ScalarResult::Int(_) => unreachable!(),
                }
                self.pop_place(&place);
            }
            ir::Stmt::Print { value, is_err } => {
                let result @ ExprResult::Scalar(_) = self.lower_expr_result(value, None) else {
                    unreachable!("strings are always scalar")
                };
                self.push_result(&result);
                self.push_intr_call(
                    if *is_err {
                        instructions::Intrinsic::Eprint
                    } else {
                        instructions::Intrinsic::Print
                    },
                    None,
                );
            }
            ir::Stmt::Assign(place, value) => {
                let place = self.lower_place(place);
                let value = self.lower_expr_result(value, Some(&place));
                if value != self.load_place(&place) {
                    self.store_place(place, &value);
                }
            }
            ir::Stmt::PanicIf(value) => {
                let value = self.lower_expr_result(value, None);
                let ExprResult::Scalar(ScalarResult::Int(value)) = value else {
                    let ExprResult::Scalar(scalar) = value else {
                        unreachable!("should be a scalar for panic if")
                    };
                    self.panic_if(&scalar);
                    return;
                };
                if value != 0 {
                    self.panic();
                    return;
                }
            }
            ir::Stmt::Loop(..) => todo!("loop"),
            ir::Stmt::Break(_) => todo!("break"),
            ir::Stmt::If(condition, then_branch, else_branch) => {
                let ExprResult::Scalar(scalar) = self.lower_expr_result(condition, None) else {
                    unreachable!("if condition should be a scalar")
                };
                if let ScalarResult::Int(n) = scalar {
                    let stmts = if n == 0 { else_branch } else { then_branch };
                    for stmt in stmts {
                        self.lower_stmt(stmt);
                    }
                    return;
                }
                if then_branch.is_empty() && else_branch.is_empty() {
                    return;
                }
                let cond_jump = {
                    let reg = self.force_scalar_in_reg(&scalar);
                    let cond_jump = self.push_instr_offset(instructions::Instr::JumpIfFalse(
                        reg,
                        instructions::JumpOffset(0),
                    ));
                    self.release_registers();
                    cond_jump
                };
                for stmt in then_branch {
                    self.lower_stmt_full(stmt);
                }
                let end_jump =
                    self.push_instr_offset(instructions::Instr::Jump(instructions::JumpOffset(0)));
                self.patch_jump_current(cond_jump);
                for stmt in else_branch {
                    self.lower_stmt_full(stmt);
                }
                self.patch_jump_current(end_jump);
            }
            ir::Stmt::Match(_) => todo!("match"),
            ir::Stmt::ReadLine(_) => todo!("read_line"),
            ir::Stmt::Alloc(..) => todo!("alloc"),
        }
    }
    fn lower(mut self) {
        for local in &self.program.bodies[self.id].locals {
            let place = self.create_local_for(&self.codegen.type_repr(&local.ty));
            self.locals.push(place);
        }
        self.local_reg_end = self.next_reg;
        for stmt in &self.program.bodies[self.id].body {
            self.lower_stmt_full(stmt);
        }
        if !self.panic_jumps.is_empty() {
            let offset = self.current_jump_offset();
            self.push_intr_call(instructions::Intrinsic::Panic, None);
            for jump in std::mem::take(&mut self.panic_jumps) {
                self.patch_jump(jump, offset);
            }
        }
        self.result_function.registers = self.max_reg;
        self.codegen.result.functions[self.function] = self.result_function;
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
        CodegenFunction::new(body_id, id, self, program).lower();
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
