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

struct PlaceRepr {
    place: CodegenPlace,
    repr: Repr,
}
enum CodegenPlace {
    Reg(RegWindow),
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
    fn push_intr_call(&mut self, instrinsic: instructions::Intrinsic, result: Option<PlaceRepr>) {
        self.result_function
            .instrs
            .push(instructions::Instr::CallIntrinisic(instrinsic));
        if let Some(place) = result {
            self.pop_place(place);
        }
    }
    fn push_reg_to_stack(&mut self, reg: instructions::Reg) {
        self.push_instr(instructions::Instr::Push(reg));
    }
    fn pop_place(&mut self, place: PlaceRepr) {
        let CodegenPlace::Reg(regs) = place.place;
        for reg in regs.into_iter().rev() {
            self.push_instr(instructions::Instr::Pop(reg));
        }
    }
    fn add_imm(&mut self, dst: instructions::Reg, src: instructions::Reg, value: i16) {
        self.push_instr(instructions::Instr::AddImm {
            dst: dst,
            src1: src,
            src2: value,
        });
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
