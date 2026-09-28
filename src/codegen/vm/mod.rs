use std::collections::HashMap;

use crate::{
    codegen::{classify_locals, vm::expr::Conditional},
    index_vec::IndexVec,
    ir,
    typed_ast::FieldId,
    types::CaseId,
    vm::instructions::{self, FunctionId},
};
mod expr;
mod stmt;
enum JumpIf {
    Zero(instructions::Reg),
    NotZero(instructions::Reg),
}
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
    #[track_caller]
    fn size_as_u16(&self) -> u16 {
        self.size.try_into().expect("too big")
    }
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

#[derive(Clone, Debug, Copy)]
enum SimplePlace {
    Reg(instructions::Reg),
    Indexed(instructions::Reg, instructions::Reg),
    ConstIndexed(instructions::Reg, u16),
}
impl From<SimplePlace> for ScalarResult {
    fn from(value: SimplePlace) -> Self {
        match value {
            SimplePlace::ConstIndexed(base, index) => ScalarResult::ConstIndex(base, index),
            SimplePlace::Indexed(base, index) => ScalarResult::Index(base, index),
            SimplePlace::Reg(reg) => ScalarResult::Reg(reg),
        }
    }
}
struct PlaceRepr {
    place: CodegenPlace,
    repr: Repr,
}
enum CodegenPlace {
    Reg(RegWindow),
}
#[derive(Clone, Debug)]
enum LoweredPlace {
    Simple(SimplePlace),
    Tuple(Vec<SimplePlace>),
}
impl LoweredPlace {
    fn as_simple_place(&self) -> Option<SimplePlace> {
        let Self::Simple(place) = self else {
            return None;
        };
        Some(*place)
    }
}
#[derive(PartialEq, Eq, Debug)]
enum ScalarResult {
    Reg(instructions::Reg),
    Index(instructions::Reg, instructions::Reg),
    ConstIndex(instructions::Reg, u16),
    Func(instructions::FunctionId),
    Int(i64),
}
impl ScalarResult {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ScalarResult::Reg(_) => None,
            ScalarResult::Index(_, _) | ScalarResult::ConstIndex(..) => None,
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
    Ult,
    Gt,
    Eq,
    And,
    Or,
}
#[derive(Clone, Copy)]
struct RegWindow {
    base: instructions::Reg,
    size: u16,
}
impl RegWindow {
    fn into_iter(self) -> impl ExactSizeIterator<Item = instructions::Reg> + DoubleEndedIterator {
        let base = self.base.into_u16();
        (base..base + self.size).map(instructions::Reg::new)
    }
    fn offset_by(self, offset: u16) -> Self {
        Self {
            base: instructions::Reg::new(self.base.into_u16() + offset),
            size: self.size,
        }
    }
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
    regs: RegWindow,
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
    loop_labels: HashMap<ir::LoopLabel, (instructions::JumpOffset, Vec<usize>)>,
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
        let mut this = Self {
            loop_labels: HashMap::new(),
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
        };

        for local in &this.program.bodies[id].locals {
            let repr = this.codegen.type_repr(&local.ty, program, &this.args);
            let size = repr.size.try_into().expect("too big");
            let regs = this.reserve_registers(size);
            this.locals.push(LocalInfo {
                regs: RegWindow { base: regs, size },
                repr,
            });
        }
        this.local_reg_end = this.next_reg;
        this
    }
    fn reserve_registers(&mut self, count: u16) -> instructions::Reg {
        let mut reg = None;
        for _ in 0..count {
            reg.get_or_insert(self.reserve_register());
        }
        reg.unwrap_or(instructions::Reg::new(0))
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
            Some(&LoweredPlace::Tuple(vec![
                SimplePlace::Reg(left_reg),
                SimplePlace::Reg(right_reg),
            ])),
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
        let result_place = result_place.and_then(|place| place.as_simple_place());
        let (dst, dst_reg) = self.reg_for_simple_place_dest_opt(result_place);
        let src1 = self.force_scalar_in_reg(&left);
        let src2 = self.force_scalar_in_reg(&right);
        let instr = match op {
            BinaryOpInstr::Add => instructions::Instr::Add {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Sub => instructions::Instr::Sub {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Div => instructions::Instr::Div {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Mul => instructions::Instr::Mul {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Lt => instructions::Instr::LesserThan {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Gt => instructions::Instr::GreaterThan {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Eq => instructions::Instr::Equals {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::And => instructions::Instr::And {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Or => instructions::Instr::Or {
                dst: dst_reg,
                src1,
                src2,
            },
            BinaryOpInstr::Ult => instructions::Instr::LesserThanUnsigned {
                dst: dst_reg,
                src1,
                src2,
            },
        };
        self.push_instr(instr);
        self.store_reg_for_simple_place(dst, dst_reg);
        ExprResult::Scalar(dst.into())
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
            ir::BinaryOp::InBounds => {
                self.eval_binary_op(BinaryOpInstr::Ult, result_place, &left, &right)
            }
        }
    }
    fn reg_for_simple_place_dest(&mut self, place: SimplePlace) -> instructions::Reg {
        match place {
            SimplePlace::Reg(reg) => reg,
            SimplePlace::Indexed(..) => self.reserve_register(),
            SimplePlace::ConstIndexed(..) => self.reserve_register(),
        }
    }
    fn store_reg_for_simple_place(&mut self, place: SimplePlace, reg: instructions::Reg) {
        match place {
            SimplePlace::Reg(_) => (),
            SimplePlace::Indexed(base, index) => {
                self.store_index(base, index, reg);
            }
            SimplePlace::ConstIndexed(base, index) => {
                self.store_index_const(base, index, reg);
            }
        }
    }
    fn reg_for_simple_place_dest_opt(
        &mut self,
        place: Option<SimplePlace>,
    ) -> (SimplePlace, instructions::Reg) {
        if let Some(place) = place {
            (place, self.reg_for_simple_place_dest(place))
        } else {
            let reg = self.reserve_register();
            (SimplePlace::Reg(reg), reg)
        }
    }
    fn codegen_imm_store(&mut self, place: CodegenPlace, src: i64) {
        match place {
            CodegenPlace::Reg(RegWindow { base, size: _ }) => {
                self.load_immediate(base, src);
            }
        }
    }
    fn lower_codegen_place(&self, place: &ir::Place) -> (CodegenPlace, Repr) {
        match place {
            ir::Place::Local(local) => {
                let local_info = &self.locals[*local];
                (CodegenPlace::Reg(local_info.regs), local_info.repr.clone())
            }
            ir::Place::Field(place, field_id) => {
                let (place, repr) = self.lower_codegen_place(place);
                let CodegenPlace::Reg(regs) = place;
                let ReprKind::Tuple(fields) = repr.kind else {
                    unreachable!()
                };

                let mut base = usize::from(regs.base.into_u16());
                for (index, repr) in fields.iter_enumerated() {
                    if index == *field_id {
                        break;
                    }
                    base += repr.size;
                }

                let base = instructions::Reg::new(base.try_into().expect("too big"));
                let repr = { fields.into_vec() }.swap_remove(field_id.into_usize());
                (
                    CodegenPlace::Reg(RegWindow {
                        base,
                        size: repr.size.try_into().expect("too big"),
                    }),
                    repr,
                )
            }
            ir::Place::Deref(place) => todo!(),
            ir::Place::Downcast(place, case_id) => {
                let (place, repr) = self.lower_codegen_place(place);
                let CodegenPlace::Reg(regs) = place;
                let ReprKind::Tuple(fields) = repr.kind else {
                    unreachable!()
                };
                let [_, union] = fields.into_vec().try_into().expect("should be a 2 tuple");
                let ReprKind::Union(case_reprs) = union.kind else {
                    unreachable!()
                };
                let repr = { case_reprs.into_vec() }.swap_remove(case_id.into_usize());
                let regs = RegWindow {
                    base: regs.offset_by(1).base,
                    size: repr.size_as_u16(),
                };
                (CodegenPlace::Reg(regs), repr)
            }
            ir::Place::Index(place, expr) => todo!(),
        }
    }
    fn function_id(&mut self, id: ir::BodyId, args: Vec<ir::Type>) -> FunctionId {
        let reprs = args
            .iter()
            .map(|arg| self.codegen.type_repr(arg, self.program, &self.args))
            .collect();
        self.codegen
            .function_for(Instance { id, args: reprs }, args.clone(), self.program)
    }
    fn eval_imm_constant(&mut self, constant: &ir::Constant) -> i64 {
        match constant {
            ir::Constant::Int(value) => *value,
            &ir::Constant::Bool(value) => value.into(),
            ir::Constant::Function(body_id, args) => {
                self.function_id(*body_id, args.clone()).as_u32().into()
            }
            ir::Constant::String(symbol) => {
                symbol.with_str(|symbol| self.codegen.string_index(symbol))
            }
            &ir::Constant::Char(value) => u32::from(value).into(),
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
            ir::ExprKind::Len(_) => {
                todo!("nah")
            }
            ir::ExprKind::Discriminant(_) => todo!("discriminant"),
            ir::ExprKind::Aggregate(kind, fields) => match kind {
                ir::AggregateKind::Tuple => ExprResult::Tuple({
                    let mut results = Vec::new();
                    for field in fields {
                        results.extend(self.lower_expr_result(field, None).into_scalars());
                    }
                    results
                }),
                ir::AggregateKind::Record(..) => todo!("named"),
                ir::AggregateKind::Variant(_, case, _) => ExprResult::Tuple({
                    let mut results = vec![ScalarResult::Int(case.into_u32().into())];
                    for field in fields {
                        results.extend(self.lower_expr_result(field, result).into_scalars());
                    }
                    results
                }),
            },
            ir::ExprKind::BinaryOp(op, left, right) => {
                let left = self.lower_expr_result(left, None).into_single_scalar();
                let right = self.lower_expr_result(right, None).into_single_scalar();
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
                let result = result.and_then(|place| place.as_simple_place());
                let (dst, dst_reg) = self.reg_for_simple_place_dest_opt(result);
                self.push_instr(instructions::Instr::Not {
                    dst: dst_reg,
                    src: reg,
                });
                match dst {
                    SimplePlace::Reg(_) => {}
                    SimplePlace::Indexed(base, index) => {
                        self.store_index(base, index, dst_reg);
                    }
                    SimplePlace::ConstIndexed(base, index) => {
                        self.store_index_const(base, index, dst_reg);
                    }
                }
                ExprResult::Scalar(dst.into())
            }
        }
    }
    fn store_index(
        &mut self,
        base: instructions::Reg,
        offset: instructions::Reg,
        src: instructions::Reg,
    ) {
        self.push_instr(instructions::Instr::StoreIndex { base, offset, src });
    }
    fn store_index_const(&mut self, base: instructions::Reg, offset: u16, src: instructions::Reg) {
        self.push_instr(instructions::Instr::StoreIndexImm { base, offset, src });
    }
    fn load_index_imm(&mut self, dst: instructions::Reg, src: instructions::Reg, index: u16) {
        self.push_instr(instructions::Instr::LoadIndexImm {
            dst,
            src,
            offset: index,
        });
    }
    fn load_index(
        &mut self,
        dst: instructions::Reg,
        base: instructions::Reg,
        offset: instructions::Reg,
    ) {
        self.push_instr(instructions::Instr::LoadIndex { dst, base, offset });
    }
    fn eval_load_index(
        &mut self,
        base: instructions::Reg,
        offset: instructions::Reg,
    ) -> instructions::Reg {
        let dst = self.reserve_register();
        self.push_instr(instructions::Instr::LoadIndex { dst, base, offset });
        dst
    }
    fn push_instr(&mut self, instr: instructions::Instr) {
        self.result_function.instrs.push(instr);
    }
    fn push_jump_if(&mut self, jump_if: JumpIf) -> usize {
        self.push_instr_offset(match jump_if {
            JumpIf::NotZero(reg) => {
                instructions::Instr::JumpIfNotZero(reg, instructions::JumpOffset(0))
            }
            JumpIf::Zero(reg) => instructions::Instr::JumpIfZero(reg, instructions::JumpOffset(0)),
        })
    }
    fn push_instr_offset(&mut self, instr: instructions::Instr) -> usize {
        let offset = self.result_function.instrs.len();
        self.push_instr(instr);
        offset
    }
    fn add_const(&mut self, value: i64) -> instructions::Const {
        let ints = &mut self.codegen.result.ints;
        let index = ints.len().try_into().expect("too many ints");
        ints.push(value);
        instructions::Const(index)
    }
    fn load_immediate(&mut self, reg: instructions::Reg, value: i64) {
        if let Ok(value) = value.try_into() {
            self.push_instr(instructions::Instr::LoadImmediate(reg, value));
            return;
        }
        let value = self.add_const(value);
        self.push_instr(instructions::Instr::LoadConst(reg, value));
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
    }
    fn push_scalar_on_stack(&mut self, value: &ScalarResult) {
        let reg = match value.as_i64() {
            Some(value) => {
                let value = self.add_const(value);
                self.push_instr(instructions::Instr::PushConst(value));
                return;
            }
            None => self.force_scalar_in_reg(value),
        };
        self.push_instr(instructions::Instr::Push(reg));
    }
    fn push_reg_to_stack(&mut self, reg: instructions::Reg) {
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
            &LoweredPlace::Simple(simple) => ExprResult::Scalar(simple.into()),
            LoweredPlace::Tuple(fields) => {
                ExprResult::Tuple(fields.iter().map(|&field| field.into()).collect())
            }
        }
    }
    fn pop_simple_place(&mut self, place: SimplePlace) {
        match place {
            SimplePlace::Reg(reg) => {
                self.push_instr(instructions::Instr::Pop(reg));
            }
            SimplePlace::Indexed(base, index) => {
                let reg = self.reserve_register();
                self.push_instr(instructions::Instr::Pop(reg));
                self.store_index(base, index, reg);
            }
            SimplePlace::ConstIndexed(base, index) => {
                let reg = self.reserve_register();
                self.push_instr(instructions::Instr::Pop(reg));
                self.store_index_const(base, index, reg);
            }
        }
    }
    fn pop_place(&mut self, place: PlaceRepr) {
        let CodegenPlace::Reg(regs) = place.place;
        for reg in regs.into_iter().rev() {
            self.push_instr(instructions::Instr::Pop(reg));
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
    fn store_scalar_in_reg(&mut self, dst: instructions::Reg, value: &ScalarResult) {
        match value {
            &ScalarResult::Func(func) => {
                self.load_immediate(
                    dst,
                    func.into_usize().try_into().expect("too many functions"),
                );
            }
            &ScalarResult::ConstIndex(base, index) => {
                self.load_index_imm(dst, base, index);
            }
            &ScalarResult::Index(base, index) => {
                self.load_index(dst, base, index);
            }
            &ScalarResult::Int(value) => {
                self.load_immediate(dst, value);
            }
            &ScalarResult::Reg(src) => {
                self.push_move(dst, src);
            }
        }
    }
    fn add_imm(&mut self, dst: instructions::Reg, src: instructions::Reg, value: i16) {
        self.push_instr(instructions::Instr::AddImm {
            dst: dst,
            src1: src,
            src2: value,
        });
    }
    fn lower_place(&mut self, place: &ir::Place) -> (LoweredPlace, Repr) {
        match place {
            ir::Place::Local(local) => {
                let local_info = &self.locals[*local];
                (
                    LoweredPlace::Tuple({
                        local_info.regs.into_iter().map(SimplePlace::Reg).collect()
                    }),
                    local_info.repr.clone(),
                )
            }
            ir::Place::Field(place, field_id) => {
                let (base_place, repr) = self.lower_place(place);
                self.project_field(base_place, *field_id, repr)
            }
            ir::Place::Deref(place) => {
                let (base_place, repr) = self.lower_place(place);
                let ReprKind::Scalar = repr.kind else {
                    unreachable!("should  be a scalar")
                };
                todo!()
            }
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
            ir::Place::Index(base, index) => {
                let ir::Type::Array(ty) =
                    base.type_of(&self.program.bodies[self.id], &self.program.type_defs)
                else {
                    unreachable!("should be an array")
                };
                let elem_repr = self.codegen.type_repr(&ty, self.program, &self.args);
                let (base_place, repr) = self.lower_place(base);
                let ReprKind::Scalar = repr.kind else {
                    unreachable!("should be a pointer")
                };
                let base = self.load_place(&base_place, &repr).into_single_scalar();
                let index = self.lower_expr_result(index, None).into_single_scalar();
                let base = {
                    let dst = self.reserve_register();
                    let base = self.force_scalar_in_reg(&base);
                    self.load_index_imm(dst, base, 0);
                    dst
                };
                match &elem_repr.kind {
                    ReprKind::Scalar => (
                        LoweredPlace::Simple(match index {
                            ScalarResult::Int(value) if let Ok(value) = value.try_into() => {
                                SimplePlace::ConstIndexed(base, value)
                            }
                            _ => SimplePlace::Indexed(base, self.force_scalar_in_reg(&index)),
                        }),
                        elem_repr,
                    ),
                    ReprKind::Tuple(_) | ReprKind::Union(_) => {
                        let indices = {
                            let index = match index {
                                ScalarResult::Int(value)
                                    if let Ok::<u16, _>(value) = value.try_into() =>
                                {
                                    Ok::<u16, _>(value)
                                }
                                _ => Err(self.force_scalar_in_reg(&index)),
                            };
                            let mut indices = Vec::new();
                            for i in 0..elem_repr.size {
                                if i == 0 {
                                    indices.push(index);
                                } else {
                                    match index {
                                        Ok(index) => {
                                            if let Ok(i) = i.try_into()
                                                && let Some(index) = index.checked_add(i)
                                            {
                                                indices.push(Ok(index));
                                            } else {
                                                let i: i64 = i.try_into().expect("too big");
                                                let reg = self.reserve_register();
                                                self.load_immediate(reg, i64::from(index) + i);
                                                indices.push(Err(reg))
                                            }
                                        }
                                        Err(index) => {
                                            let reg = self.reserve_register();
                                            self.push_instr(instructions::Instr::AddImm {
                                                dst: reg,
                                                src1: index,
                                                src2: i as _,
                                            });
                                            indices.push(Err(reg))
                                        }
                                    };
                                }
                            }
                            indices
                        };
                        (
                            LoweredPlace::Tuple(
                                indices
                                    .into_iter()
                                    .map(|index| match index {
                                        Ok(index) => SimplePlace::ConstIndexed(base, index),
                                        Err(index) => SimplePlace::Indexed(base, index),
                                    })
                                    .collect(),
                            ),
                            elem_repr,
                        )
                    }
                }
            }
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
        | instructions::Instr::JumpIfNotZero(_, offset)
        | instructions::Instr::JumpIfZero(_, offset)) = instr
        else {
            panic!("cannot patch non jump instruction {instr:?} at {instr_index}")
        };
        *offset = new_offset;
    }
    fn codegen_panic_if(&mut self, condition: Conditional) {
        let index = self.push_instr_offset(match condition {
            Conditional::Bool(false) => {
                return;
            }
            Conditional::Bool(true) => instructions::Instr::Jump(instructions::JumpOffset(0)),
            Conditional::Not(reg) => {
                instructions::Instr::JumpIfZero(reg, instructions::JumpOffset(0))
            }
            Conditional::Reg(reg) => {
                instructions::Instr::JumpIfNotZero(reg, instructions::JumpOffset(0))
            }
        });
        self.panic_jumps.push(index);
    }
    fn panic(&mut self) {
        let index = self.push_instr_offset(instructions::Instr::Jump(instructions::JumpOffset(0)));
        self.panic_jumps.push(index);
    }
    fn lower(mut self) {
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
            ir::Type::Array(_) => SCALAR_REPR,
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
                    ir::TypeDef::Struct(struct_def) => {
                        todo!("handle structs")
                    }
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
