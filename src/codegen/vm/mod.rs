use std::collections::HashMap;

use crate::{
    codegen::classify_locals, index_vec::IndexVec, ir, typed_ast::FieldId, types::CaseId,
    vm::instructions,
};
#[derive(PartialEq, Eq, Hash, Clone)]
struct Instance {
    id: ir::BodyId,
    args: Vec<Repr>,
}
impl Instance {
    fn new(id: ir::BodyId) -> Self {
        Self {
            id,
            args: Vec::new(),
        }
    }
    fn with_args(id: ir::BodyId, args: Vec<Repr>) -> Self {
        Self { id, args }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ReprKind {
    Scalar,
    Tuple(IndexVec<FieldId, Repr>),
    Union(IndexVec<CaseId, Repr>),
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Repr {
    size: usize,
    kind: ReprKind,
}
impl Repr {
    fn single_tuple(field: Self) -> Self {
        Self::tuple([field])
    }
    fn pair(first: Self, second: Self) -> Self {
        Self::tuple([first, second])
    }
    fn tuple(fields: impl IntoIterator<Item = Self>) -> Self {
        let fields = fields.into_iter().collect::<IndexVec<_, _>>();
        let size = fields.iter().map(|field| field.size).sum();
        Self {
            size,
            kind: ReprKind::Tuple(fields),
        }
    }
    fn union(cases: impl IntoIterator<Item = Self>) -> Self {
        let fields = cases.into_iter().collect::<IndexVec<_, _>>();
        let size = fields.iter().map(|field| field.size).max().unwrap_or(0);
        Self {
            size,
            kind: ReprKind::Union(fields),
        }
    }
}
const SCALAR_REPR: Repr = Repr {
    size: 1,
    kind: ReprKind::Scalar,
};
const UNIT_REPR: Repr = Repr {
    size: 0,
    kind: ReprKind::Tuple(IndexVec::new()),
};

#[derive(Clone, Debug)]
enum LoweredPlace {
    Reg(instructions::Reg),
    Tuple(Vec<instructions::Reg>),
}
impl LoweredPlace {
    pub fn regs(&self) -> Vec<instructions::Reg> {
        match self {
            &Self::Reg(reg) => vec![reg],
            Self::Tuple(fields) => fields.clone(),
        }
    }
}
#[derive(PartialEq, Eq, Debug)]
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
    And,
    Or,
}
#[derive(PartialEq, Eq, Debug)]
enum ExprResult {
    Scalar(ScalarResult),
    Tuple(Vec<ScalarResult>),
}
impl ExprResult {
    fn pair(first: impl Into<ScalarResult>, second: impl Into<ScalarResult>) -> Self {
        Self::Tuple(vec![first.into(), second.into()])
    }
    fn scalars(&self) -> Vec<&ScalarResult> {
        match self {
            Self::Scalar(scalar) => vec![scalar],
            Self::Tuple(elements) => {
                let mut output = Vec::new();
                for element in elements {
                    output.push(element);
                }
                output
            }
        }
    }
    fn into_scalars(self) -> Vec<ScalarResult> {
        match self {
            Self::Scalar(scalar) => vec![scalar],
            Self::Tuple(elements) => elements,
        }
    }
    fn into_single_scalar(self) -> ScalarResult {
        match self {
            Self::Scalar(scalar) => scalar,
            Self::Tuple(elements) => { elements }.swap_remove(0),
        }
    }
}
struct LocalInfo {
    place: LoweredPlace,
    repr: Repr,
}
struct CodegenFunction<'a> {
    id: ir::BodyId,
    args: Vec<ir::Type>,
    function: instructions::FunctionId,
    result_function: instructions::Function,
    codegen: &'a mut Codegen,
    program: &'a ir::Program,
    locals: IndexVec<ir::Local, LocalInfo>,
    local_reg_end: u16,
    next_reg: u16,
    max_reg: u16,
    panic_jumps: Vec<usize>,
    _local_info: IndexVec<ir::Local, super::AssignCount>,
}
impl<'a> CodegenFunction<'a> {
    fn new(
        id: ir::BodyId,
        args: Vec<ir::Type>,
        function: instructions::FunctionId,
        codegen: &'a mut Codegen,
        program: &'a ir::Program,
    ) -> Self {
        let body = &program.bodies[id];
        Self {
            args,
            _local_info: classify_locals(body),
            id: id,
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
            ir::OverflowOp::Mul => instructions::Intrinsic::MulWithOverflow,
        };
        self.push_scalar_on_stack(&left);
        self.push_scalar_on_stack(&right);
        if let Some(result_place) = result_place {
            self.push_intr_call(instrinsic, Some(result_place));
            return self.load_place(&result_place, &Repr::pair(SCALAR_REPR, SCALAR_REPR));
        }
        let left_reg = self.reserve_register();
        let right_reg = self.reserve_register();
        self.push_intr_call(
            instrinsic,
            Some(&LoweredPlace::Tuple(vec![left_reg, right_reg])),
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
            BinaryOpInstr::Div => instructions::Instr::Div { dst, src1, src2 },
            BinaryOpInstr::Mul => instructions::Instr::Mul { dst, src1, src2 },
            BinaryOpInstr::Lt => instructions::Instr::LesserThan { dst, src1, src2 },
            BinaryOpInstr::Gt => instructions::Instr::GreaterThan { dst, src1, src2 },
            BinaryOpInstr::Eq => instructions::Instr::Equals { dst, src1, src2 },
            BinaryOpInstr::And => instructions::Instr::And { dst, src1, src2 },
            BinaryOpInstr::Or => instructions::Instr::Or { dst, src1, src2 },
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
            ir::BinaryOp::Multiply => {
                self.eval_binary_op(BinaryOpInstr::Mul, result_place, &left, &right)
            }
            ir::BinaryOp::MultiplyWithOverflow => {
                self.eval_overflow_op(result_place, ir::OverflowOp::Mul, &left, &right)
            }
            ir::BinaryOp::Divide => {
                self.eval_binary_op(BinaryOpInstr::Div, result_place, &left, &right)
            }
            ir::BinaryOp::BitwiseAnd => {
                self.eval_binary_op(BinaryOpInstr::And, result_place, &left, &right)
            }
            ir::BinaryOp::BitwiseOr => {
                self.eval_binary_op(BinaryOpInstr::Or, result_place, &left, &right)
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
                ir::Constant::Function(id, ty_args) => {
                    let args = ty_args
                        .iter()
                        .map(|arg| self.codegen.type_repr(arg, self.program, &self.args))
                        .collect();
                    let instance = Instance::with_args(*id, args);
                    let id = self
                        .codegen
                        .function_for(instance, ty_args.clone(), self.program);
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
                let (place, repr) = self.lower_place(place);
                self.load_place(&place, &repr)
            }
            ir::ExprKind::Len(_) => todo!("len"),
            ir::ExprKind::Discriminant(_) => todo!("discriminant"),
            ir::ExprKind::Aggregate(kind, fields) => match kind {
                ir::AggregateKind::Tuple => ExprResult::Tuple({
                    let mut results = Vec::new();
                    for field in fields {
                        results.extend(self.lower_expr_result(field, None).into_scalars());
                    }
                    results
                }),
                ir::AggregateKind::Named => todo!("named"),
                ir::AggregateKind::Variant(_, case, _) => ExprResult::Tuple({
                    let mut results = vec![ScalarResult::Int(case.into_u32().into())];
                    for field in fields {
                        results.extend(self.lower_expr_result(field, result).into_scalars());
                    }
                    results
                }),
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
    fn load_immediate(&mut self, reg: instructions::Reg, value: i64) {
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
                    self.push_scalar_on_stack(element);
                }
            }
        }
    }
    fn project_field(
        &self,
        base_place: LoweredPlace,
        field_id: FieldId,
        repr: Repr,
    ) -> (LoweredPlace, Repr) {
        let ReprKind::Tuple(fields) = repr.kind else {
            unreachable!("should be a tuple but got {:?}", repr)
        };
        let mut repr_fields = fields.into_vec();
        let LoweredPlace::Tuple(fields) = base_place else {
            unreachable!("should be a tuple")
        };
        let mut offset = 0;
        for i in 0..field_id.into_usize() {
            offset += repr_fields[i].size;
        }
        let repr = repr_fields.swap_remove(field_id.into_usize());
        let fields = fields[offset..][..repr.size].to_vec();
        (LoweredPlace::Tuple(fields), repr)
    }
    #[track_caller]
    fn load_place(&self, place: &LoweredPlace, _: &Repr) -> ExprResult {
        match place {
            &LoweredPlace::Reg(reg) => ExprResult::Scalar(ScalarResult::Reg(reg)),
            LoweredPlace::Tuple(fields) => ExprResult::Tuple(
                fields
                    .iter()
                    .map(|field| ScalarResult::Reg(*field))
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
                for &reg in fields.into_iter().rev() {
                    self.push_instr(instructions::Instr::Pop(reg));
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
                self.load_immediate(
                    reg,
                    func.into_usize().try_into().expect("too many functions"),
                );
            }
            &ScalarResult::Int(value) => {
                self.load_immediate(reg, value);
            }
            &ScalarResult::Reg(src) => {
                self.push_move(reg, src);
            }
        }
    }
    #[track_caller]
    fn store_place(&mut self, place: &LoweredPlace, result: &ExprResult) {
        for (reg, scalar) in place.regs().into_iter().zip(result.scalars()) {
            self.store_scalar_in_reg(reg, scalar);
        }
    }
    fn lower_place(&self, place: &ir::Place) -> (LoweredPlace, Repr) {
        match place {
            ir::Place::Local(local) => {
                let local_info = &self.locals[*local];
                (local_info.place.clone(), local_info.repr.clone())
            }
            ir::Place::Field(place, field_id) => {
                let (base_place, repr) = self.lower_place(place);
                self.project_field(base_place, *field_id, repr)
            }
            ir::Place::Deref(_) => todo!(),
            ir::Place::Downcast(place, case_id) => {
                let (base_place, repr) = self.lower_place(place);
                let ReprKind::Tuple(fields) = repr.kind else {
                    unreachable!("should be a tuple for variant")
                };
                let [_, payload] = fields.into_vec().try_into().expect("should have 2 fields");

                let ReprKind::Union(cases) = payload.kind else {
                    unreachable!("should be a union")
                };
                let LoweredPlace::Tuple(fields) = base_place else {
                    unreachable!("should be a tuple")
                };
                let mut cases = cases.into_vec();
                let repr = cases.remove(case_id.into_usize());
                let fields = fields[1..].to_vec();
                (LoweredPlace::Tuple(fields), repr)
            }
            ir::Place::Index(..) => todo!(),
        }
    }
    fn create_local_for(&mut self, repr: &Repr, regs: Vec<instructions::Reg>) -> LoweredPlace {
        match &repr.kind {
            ReprKind::Scalar => {
                let [reg] = regs.try_into().expect("should be single reg");
                LoweredPlace::Reg(reg)
            }
            ReprKind::Tuple(_) | ReprKind::Union(_) => LoweredPlace::Tuple(regs),
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
                let (place, _) = self.lower_place(return_place);
                let function = self.lower_expr_result(callee, None).into_single_scalar();
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
                let result = self.lower_expr_result(value, None);
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
                let (place, repr) = self.lower_place(place);
                let value = self.lower_expr_result(value, Some(&place));
                if value != self.load_place(&place, &repr) {
                    self.store_place(&place, &value);
                }
            }
            ir::Stmt::PanicIf(value) => {
                let value = self.lower_expr_result(value, None);
                let value = value.into_single_scalar();
                let ScalarResult::Int(value) = value else {
                    self.panic_if(&value);
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
                let scalar = self.lower_expr_result(condition, None).into_single_scalar();
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
            let repr = self.codegen.type_repr(&local.ty, self.program, &self.args);
            let regs = (0..repr.size)
                .map(|_| self.reserve_register())
                .collect::<Vec<_>>();
            let place = self.create_local_for(&repr, regs);
            self.locals.push(LocalInfo { place, repr });
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
    function_map: HashMap<Instance, (instructions::FunctionId, Vec<ir::Type>)>,
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
    fn type_repr(&self, ty: &ir::Type, program: &ir::Program, args: &[ir::Type]) -> Repr {
        match ty {
            ir::Type::Int
            | ir::Type::Bool
            | ir::Type::String
            | ir::Type::Char
            | ir::Type::Function(..) => SCALAR_REPR,
            ir::Type::Never => UNIT_REPR,
            ir::Type::Param(index) => self.type_repr(&args[*index as usize], program, args),
            ir::Type::Tuple(fields) => {
                Repr::tuple(fields.iter().map(|ty| self.type_repr(ty, program, args)))
            }
            ir::Type::Array(_) => todo!(),
            ir::Type::Named(id, args) => {
                let args = args
                    .iter()
                    .cloned()
                    .map(|mut arg| {
                        arg.subst(args);
                        arg
                    })
                    .collect::<Vec<_>>();
                match &program.type_defs[*id] {
                    ir::TypeDef::Struct => todo!("handle structs"),
                    ir::TypeDef::Variant(variant_def) => {
                        let reprs = variant_def.cases.iter_enumerated().map(|(_, case)| {
                            if let Some(ref field) = case.field {
                                Repr::single_tuple(self.type_repr(&field.ty, program, &args))
                            } else {
                                UNIT_REPR
                            }
                        });
                        Repr::pair(SCALAR_REPR, Repr::union(reprs))
                    }
                }
            }
            ir::Type::Box(_) => SCALAR_REPR,
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
        instance: Instance,
        args: Vec<ir::Type>,
        program: &ir::Program,
    ) -> instructions::FunctionId {
        if let Some(&(id, _)) = self.function_map.get(&instance) {
            return id;
        }
        let id = self.push_function(instructions::Function {
            registers: 0,
            instrs: vec![],
        });
        self.function_map
            .insert(instance.clone(), (id, args.clone()));
        CodegenFunction::new(instance.id, args, id, self, program).lower();
        id
    }
    fn make_entrypoint_function(&mut self, program: &ir::Program) -> instructions::FunctionId {
        let instrs = if let Some(entrypoint) = program.entrypoint {
            let id = self.function_for(Instance::new(entrypoint), Vec::new(), program);
            vec![instructions::Instr::Call(id), instructions::Instr::Return]
        } else {
            vec![instructions::Instr::Return]
        };
        self.push_function(instructions::Function {
            registers: 0,
            instrs: instrs,
        })
    }
    fn lower_program(
        mut self,
        program: &ir::Program,
    ) -> (
        instructions::Program,
        instructions::FunctionId,
        HashMap<instructions::FunctionId, (Vec<ir::Type>, Instance)>,
    ) {
        let entrypoint = self.make_entrypoint_function(program);
        let program = self.result;
        let map = self
            .function_map
            .into_iter()
            .map(|(first, (id, args))| (id, (args, first)))
            .collect();
        (program, entrypoint, map)
    }
}

pub fn codegen(ir_program: ir::Program) -> (instructions::Program, instructions::FunctionId) {
    let (program, entrypoint, map) = Codegen::new().lower_program(&ir_program);
    for (i, function) in program.functions.iter_enumerated() {
        if let Some((args, instance)) = map.get(&i) {
            println!("body {} {:?}", ir_program.bodies[instance.id].name, args);
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
