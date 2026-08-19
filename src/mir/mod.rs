use std::collections::HashMap;

use crate::{
    Symbol,
    collect::CtxtRef,
    def_ids::DefId,
    define_id,
    index_vec::IndexVec,
    mir::basic_blocks::BasicBlocks,
    monomorph::collect::InstanceKind,
    resolved_ast::{Var, VarId},
    src_loc::SrcLoc,
    typed_ast::FieldId,
    types::{CaseId, GenericArgs, IntegerKind, IntegerSize, Type},
};
pub mod basic_blocks;
pub mod build;
pub mod dump;
pub mod passes;
pub mod traversal;
pub mod visitor;
pub mod well_formed;
define_id!(Local);
impl Local {
    pub const FIRST_PARAM: Self = Self(0);
}
#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub enum PlaceProjection {}
impl PlaceProjection {
    pub fn apply_projection_to_type<'ctxt>(self, _: Type<'ctxt>, _: CtxtRef<'ctxt>) -> Type<'ctxt> {
        match self {}
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Place {
    pub local: Local,
}
impl Place {
    pub fn local(local: Local) -> Self {
        Self { local }
    }
    pub fn type_of<'ctxt>(
        &self,
        _: CtxtRef<'ctxt>,
        locals: &Locals<'ctxt>,
        _: Type<'ctxt>,
    ) -> Type<'ctxt> {
        locals[self.local].ty
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum ConstValue<'ctxt> {
    ZeroSized,
    Named(DefId, GenericArgs<'ctxt>),
    Scalar(i128),
    Variant(CaseId, Option<Box<Constant<'ctxt>>>),
    Record(Box<[Constant<'ctxt>]>),
    String(Symbol),
}
impl<'ctxt> ConstValue<'ctxt> {
    fn as_scalar(&self) -> Option<i128> {
        match self {
            Self::Scalar(value) => Some(*value),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Constant<'ctxt> {
    pub ty: Type<'ctxt>,

    pub value: ConstValue<'ctxt>,
}
impl<'ctxt> Constant<'ctxt> {
    pub fn zero(ctxt: CtxtRef<'ctxt>, kind: IntegerKind) -> Self {
        match kind {
            IntegerKind::Signed(size) => Self::int(ctxt, size, 0),
            IntegerKind::Unsigned(size) => Self::uint(ctxt, size, 0),
        }
    }
    pub fn bool(ctxt: CtxtRef<'ctxt>, value: bool) -> Self {
        Self {
            ty: Type::new_bool(ctxt),
            value: ConstValue::Scalar(value as i128),
        }
    }
    pub fn integer(ctxt: CtxtRef<'ctxt>, kind: IntegerKind, value: i128) -> Self {
        Self {
            ty: Type::new_integer(ctxt, kind),
            value: ConstValue::Scalar(value),
        }
    }
    pub fn int(ctxt: CtxtRef<'ctxt>, size: IntegerSize, value: i64) -> Self {
        Self::integer(ctxt, IntegerKind::Signed(size), value.into())
    }
    pub fn char(ctxt: CtxtRef<'ctxt>, value: char) -> Self {
        Self {
            ty: Type::new_char(ctxt),
            value: ConstValue::Scalar(value as i128),
        }
    }
    pub fn uint(ctxt: CtxtRef<'ctxt>, size: IntegerSize, value: u64) -> Self {
        Self::integer(ctxt, IntegerKind::Unsigned(size), value.into())
    }
    pub const fn zero_sized(ty: Type<'ctxt>) -> Self {
        Self {
            ty,
            value: ConstValue::ZeroSized,
        }
    }
    pub fn unit(ctxt: CtxtRef<'ctxt>) -> Self {
        Self::zero_sized(Type::new_unit(ctxt))
    }
}
#[derive(Clone, Debug)]
pub enum Operand<'ctxt> {
    Load(Place),
    Constant(Constant<'ctxt>),
}
impl<'ctxt> Operand<'ctxt> {
    pub fn type_of(
        &self,
        ctxt: CtxtRef<'ctxt>,
        locals: &Locals<'ctxt>,
        return_type: Type<'ctxt>,
    ) -> Type<'ctxt> {
        match self {
            Operand::Constant(constant) => constant.ty,
            Operand::Load(place) => place.type_of(ctxt, locals, return_type),
        }
    }
}
#[derive(Clone, Debug)]
pub enum AggregateKind<'ctxt> {
    Tuple,
    NamedRecord(DefId, GenericArgs<'ctxt>),
    Variant(DefId, CaseId, GenericArgs<'ctxt>),
}
#[derive(Debug, Clone, Copy)]
pub enum OverflowOp {
    Add,
    Subtract,
    Multiply,
}
#[derive(Debug, Clone, Copy)]
pub enum BinaryOp {
    Overflow(OverflowOp),
    Wrapping(OverflowOp),
    Greater,
    Divide,
    Equals,
    Lesser,
    BitwiseAnd,
    BitwiseOr,
    ShiftLeft,
    ShiftRight,
    Offset,
}
#[derive(Clone, Debug, Copy)]
pub enum IntegerCast {
    SignExtend(IntegerSize),
    ZeroExtend(IntegerSize),
    Truncate(IntegerKind),
}
#[derive(Clone, Debug, Copy)]
pub enum CastKind {
    Transmute,
    IntegerCast(IntegerCast),
}
#[derive(Clone, Debug)]
pub enum Rvalue<'ctxt> {
    ReadLine,
    UninitZeroed(Type<'ctxt>),
    Aggregate(AggregateKind<'ctxt>, IndexVec<FieldId, Operand<'ctxt>>),
    AllocateRawArray {
        ty: Type<'ctxt>,
        count: Operand<'ctxt>,
    },
    AllocateArray(Type<'ctxt>, Vec<Operand<'ctxt>>),
    AllocateBox(Type<'ctxt>, Operand<'ctxt>),
    Use(Operand<'ctxt>),
    Call(Operand<'ctxt>, Vec<Operand<'ctxt>>),
    Binary(BinaryOp, Box<(Operand<'ctxt>, Operand<'ctxt>)>),
    AddrOf(Place),
    Cast(CastKind, Operand<'ctxt>, Type<'ctxt>),
    Len(Place),
    Discriminant(Place),
    LoadIndex(Place, Operand<'ctxt>),
    LoadField(Place, FieldId),
    LoadPayload(Place, CaseId),
    Unbox(Place),
    GcAlloc(Type<'ctxt>, Operand<'ctxt>),
}
impl<'ctxt> Rvalue<'ctxt> {
    pub fn can_remove_if_unused(&self) -> bool {
        match self {
            Self::Aggregate(..)
            | Self::Binary(..)
            | Self::Cast(..)
            | Self::Use(_)
            | Self::AddrOf(_)
            | Self::Len(_)
            | Self::Discriminant(_)
            | Self::UninitZeroed(_)
            | Self::LoadField(..)
            | Self::LoadPayload(..)
            | Self::Unbox(_) => true,
            Self::LoadIndex(..) => false,
            Self::GcAlloc(..) => false,

            Self::AllocateArray(..)
            | Self::AllocateBox(..)
            | Self::Call(..)
            | Self::AllocateRawArray { .. }
            | Self::ReadLine => false,
        }
    }

    pub fn type_of(
        &self,
        ctxt: CtxtRef<'ctxt>,
        locals: &Locals<'ctxt>,
        return_type: Type<'ctxt>,
    ) -> Type<'ctxt> {
        match self {
            Rvalue::Unbox(place) => {
                let ty = place.type_of(ctxt, locals, return_type);
                ty.as_box().expect("should be a box")
            }
            Rvalue::LoadPayload(place, case) => {
                let ty = place.type_of(ctxt, locals, return_type);
                let Some((id, _, args)) = ty.as_named() else {
                    unreachable!("Should be named")
                };

                let type_def = ctxt.type_def(id);
                let case_info = type_def.case(*case);
                case_info.payload_type(args, ctxt)
            }
            Rvalue::LoadField(place, field) => {
                let ty = place.type_of(ctxt, locals, return_type);
                ty.field_info(*field, ctxt)
                    .unwrap_or_else(|| panic!("should be a type with fields but got '{ty}'"))
                    .0
            }
            Rvalue::LoadIndex(place, _) => place
                .type_of(ctxt, locals, return_type)
                .as_array()
                .expect("should be an array"),
            Rvalue::GcAlloc(ty, _) => Type::new_raw_ptr(ctxt, *ty),
            Rvalue::ReadLine => Type::new_string(ctxt),
            &Rvalue::UninitZeroed(ty) => Type::new_uninit(ctxt, ty),
            &Rvalue::AllocateBox(ty, _) => Type::new_box(ctxt, ty),
            Rvalue::Use(operand) => operand.type_of(ctxt, locals, return_type),
            Rvalue::Len(_) => Type::new_uint(ctxt, IntegerSize::Int64),
            Rvalue::Call(operand, _) => {
                let Some(function) = operand.type_of(ctxt, locals, return_type).as_function()
                else {
                    unreachable!("Should be a function type")
                };
                function.return_type
            }
            Rvalue::Binary(op, left_and_right) => match op {
                BinaryOp::Overflow(_) => Type::pair(
                    ctxt,
                    left_and_right.0.type_of(ctxt, locals, return_type),
                    Type::new_bool(ctxt),
                ),
                BinaryOp::Wrapping(_)
                | BinaryOp::BitwiseAnd
                | BinaryOp::BitwiseOr
                | BinaryOp::ShiftLeft
                | BinaryOp::ShiftRight => left_and_right.0.type_of(ctxt, locals, return_type),
                BinaryOp::Divide => left_and_right.0.type_of(ctxt, locals, return_type),
                BinaryOp::Equals => Type::new_bool(ctxt),
                BinaryOp::Lesser | BinaryOp::Greater => Type::new_bool(ctxt),
                BinaryOp::Offset => left_and_right.0.type_of(ctxt, locals, return_type),
            },
            Rvalue::AllocateArray(element, _) => Type::new_array(ctxt, *element),
            Rvalue::AllocateRawArray { ty, .. } => Type::new_raw_array(ctxt, *ty),
            Rvalue::Aggregate(aggregate, operands) => match aggregate {
                &AggregateKind::Variant(id, _, ref args)
                | &AggregateKind::NamedRecord(id, ref args) => {
                    let name = ctxt.type_def(id).name;
                    Type::named(ctxt, id, name, args.clone())
                }
                AggregateKind::Tuple => Type::tuple_from_iter(
                    ctxt,
                    operands
                        .iter()
                        .map(|operand| operand.type_of(ctxt, locals, return_type)),
                ),
            },
            &Rvalue::Cast(.., ty) => ty,
            Rvalue::Discriminant(_) => Type::new_uint(ctxt, IntegerSize::Int64),
            Rvalue::AddrOf(_) => Type::new_uint(ctxt, IntegerSize::Int64),
        }
    }
}
#[derive(Clone, Debug)]
pub struct SwitchTarget {
    pub value: i128,
    pub target: BasicBlockId,
}
#[derive(Clone, Debug)]
pub struct SwitchTargets {
    pub targets: Vec<SwitchTarget>,
    pub otherwise: BasicBlockId,
}
pub struct TargetIterator<'a> {
    targets: std::slice::Iter<'a, SwitchTarget>,
    otherwise: Option<BasicBlockId>,
}
impl TargetIterator<'_> {
    fn len(&self) -> usize {
        self.targets.as_slice().len() + self.otherwise.is_some() as usize
    }
}
impl Iterator for TargetIterator<'_> {
    type Item = BasicBlockId;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(current) = self.targets.next() {
            return Some(current.target);
        }
        self.otherwise.take()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}
impl ExactSizeIterator for TargetIterator<'_> {
    fn len(&self) -> usize {
        self.len()
    }
}
impl DoubleEndedIterator for TargetIterator<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if let Some(value) = self.otherwise.take() {
            return Some(value);
        }
        self.targets.next_back().map(|target| target.target)
    }
}

impl SwitchTargets {
    pub fn branch_for_value(&self, value: i128) -> BasicBlockId {
        self.targets
            .iter()
            .find_map(|target| (target.value == value).then_some(target.target))
            .unwrap_or(self.otherwise)
    }
    pub fn succesors_iter(&self) -> TargetIterator<'_> {
        TargetIterator {
            targets: self.targets.iter(),
            otherwise: Some(self.otherwise),
        }
    }
}
#[derive(Clone, Debug)]
pub enum AssertKind {
    InBounds,
    Overflow(OverflowOp),
    DivideOverflow,
    DivideByZero,
}
impl AssertKind {
    pub fn negate(&self) -> bool {
        !matches!(self, Self::InBounds)
    }
}

pub struct Successors<'a>(SuccessorsIter<'a>);
enum SuccessorsIter<'a> {
    Switch(TargetIterator<'a>),
    Single(Option<BasicBlockId>),
    Leaf,
}
impl Iterator for Successors<'_> {
    type Item = BasicBlockId;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            SuccessorsIter::Leaf => None,
            SuccessorsIter::Single(block) => block.take(),
            SuccessorsIter::Switch(targets) => targets.next(),
        }
    }
}
impl DoubleEndedIterator for Successors<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            SuccessorsIter::Leaf => None,
            SuccessorsIter::Single(block) => block.take(),
            SuccessorsIter::Switch(targets) => targets.next_back(),
        }
    }
}
impl ExactSizeIterator for Successors<'_> {
    fn len(&self) -> usize {
        match &self.0 {
            SuccessorsIter::Leaf => 0,
            SuccessorsIter::Switch(targets) => targets.len(),
            SuccessorsIter::Single(target) => target.is_some() as usize,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Terminator<'ctxt> {
    pub src_info: SrcLoc,
    pub kind: TerminatorKind<'ctxt>,
}
impl<'ctxt> Terminator<'ctxt> {
    pub fn successors(&self) -> Successors<'_> {
        Successors(match self.kind {
            TerminatorKind::Assert(.., block) | TerminatorKind::Goto(block) => {
                SuccessorsIter::Single(Some(block))
            }
            TerminatorKind::Return(_) | TerminatorKind::Panic | TerminatorKind::Unreachable => {
                SuccessorsIter::Leaf
            }
            TerminatorKind::Switch(_, ref targets) => {
                SuccessorsIter::Switch(targets.succesors_iter())
            }
        })
    }
    pub fn successors_mut(&mut self) -> impl Iterator<Item = &mut BasicBlockId> {
        let (single, multiple) = match &mut self.kind {
            TerminatorKind::Goto(block) | TerminatorKind::Assert(.., block) => (Some(block), None),
            TerminatorKind::Switch(_, switch_targets) => (
                None,
                Some(
                    switch_targets
                        .targets
                        .iter_mut()
                        .map(|target| &mut target.target)
                        .chain(std::iter::once(&mut switch_targets.otherwise)),
                ),
            ),
            TerminatorKind::Unreachable => None.unzip(),
            TerminatorKind::Return(_) => None.unzip(),
            TerminatorKind::Panic => None.unzip(),
        };
        single.into_iter().chain(multiple.into_iter().flatten())
    }
}
#[derive(Clone, Debug)]
pub enum TerminatorKind<'ctxt> {
    Assert(Operand<'ctxt>, AssertKind, BasicBlockId),
    Switch(Operand<'ctxt>, SwitchTargets),
    Unreachable,
    Return(Operand<'ctxt>),
    Goto(BasicBlockId),
    Panic,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Location {
    pub block: BasicBlockId,
    pub stmt: Option<StmtId>,
}
impl Location {
    pub fn stmt(block: BasicBlockId, stmt: StmtId) -> Self {
        Self {
            block,
            stmt: Some(stmt),
        }
    }
    pub fn terminator(block: BasicBlockId) -> Self {
        Self { block, stmt: None }
    }
}
impl std::fmt::Debug for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bb{}-", self.block.0)?;
        match self.stmt {
            Some(value) => write!(f, "stmt{}", value.0),
            None => write!(f, "term"),
        }
    }
}

#[derive(Clone)]
pub struct Stmt<'ctxt> {
    pub loc: SrcLoc,
    pub kind: StmtKind<'ctxt>,
}
#[derive(Clone)]
pub enum StmtKind<'ctxt> {
    Noop,
    AssignBox(Place, Operand<'ctxt>),
    AssignField(Place, FieldId, Operand<'ctxt>),
    AssignIndex(Place, Operand<'ctxt>, Operand<'ctxt>),
    Assign(Place, Box<Rvalue<'ctxt>>),
    Print {
        value: Operand<'ctxt>,
        err: bool,
    },
    Copy {
        dst: Operand<'ctxt>,
        src: Operand<'ctxt>,
        count: Operand<'ctxt>,
    },
}
define_id!(BasicBlockId);
impl BasicBlockId {
    pub const ENTRY: Self = Self(0);
}
define_id!(StmtId);
#[derive(Default, Clone)]
pub struct BasicBlock<'ctxt> {
    pub stmts: IndexVec<StmtId, Stmt<'ctxt>>,
    pub terminator: Option<Terminator<'ctxt>>,
}
impl<'ctxt> BasicBlock<'ctxt> {
    #[track_caller]
    pub fn expect_terminator(&self) -> &Terminator<'ctxt> {
        self.terminator
            .as_ref()
            .expect("Block should have a terminator")
    }
    #[track_caller]
    pub fn expect_terminator_mut(&mut self) -> &mut Terminator<'ctxt> {
        self.terminator
            .as_mut()
            .expect("Block should have a terminator")
    }
}
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum BodySource {
    Function(DefId),
}
impl BodySource {
    pub fn def_id(self) -> DefId {
        match self {
            Self::Function(id) => id,
        }
    }
    pub fn as_instance(self) -> InstanceKind {
        match self {
            Self::Function(id) => InstanceKind::Function(id),
        }
    }
    pub fn is_child_of(self, name: Symbol, ctxt: CtxtRef) -> bool {
        ctxt.self_with_anecstors(self.def_id())
            .any(|id| ctxt.ident(id).map(|ident| ident.symbol) == Some(name))
    }
}
#[derive(Clone, Debug)]
pub enum LocalKind {
    Temp,
    Env,
    Var(Var),
    Param(Option<Var>),
}
#[derive(Clone, Debug)]
pub struct LocalInfo<'ctxt> {
    pub ty: Type<'ctxt>,
    pub kind: LocalKind,
}
#[derive(Clone)]
pub struct Body<'ctxt> {
    pub src: BodySource,
    pub return_type: Type<'ctxt>,
    pub locals: Locals<'ctxt>,
    pub block_info: BasicBlocks<'ctxt>,
}
impl<'ctxt> Body<'ctxt> {
    pub fn param_types(&self) -> impl Iterator<Item = Type<'ctxt>> {
        self.params_iter().map(|param| self.locals[param].ty)
    }
    pub fn params_iter(&self) -> impl Iterator<Item = Local> {
        self.locals
            .iter_enumerated()
            .filter_map(|(local, info)| matches!(info.kind, LocalKind::Param(_)).then_some(local))
    }
    pub fn local_for_var(&self, var_id: VarId) -> Option<Local> {
        self.locals
            .iter()
            .position(|local| {
                let (LocalKind::Var(var) | LocalKind::Param(Some(var))) = &local.kind else {
                    return false;
                };
                var.1 == var_id
            })
            .map(Local::new)
    }
    pub fn src_info(&self, loc: Location) -> SrcLoc {
        match loc.stmt {
            Some(stmt) => self.block_info.blocks()[loc.block].stmts[stmt].loc,
            None => {
                self.block_info.blocks()[loc.block]
                    .expect_terminator()
                    .src_info
            }
        }
    }
}
pub type Locals<'ctxt> = IndexVec<Local, LocalInfo<'ctxt>>;

#[derive(Default)]
pub struct Context<'ctxt> {
    pub check_well_formed: bool,
    bodies: HashMap<BodySource, Body<'ctxt>>,
    body_sources: Vec<BodySource>,
}
impl<'ctxt> Context<'ctxt> {
    pub fn new(well_formed: bool) -> Self {
        Self {
            check_well_formed: well_formed,
            ..Default::default()
        }
    }
    pub fn body_iter(&self) -> impl Iterator<Item = &Body<'ctxt>> {
        self.body_sources.iter().map(|src| &self.bodies[src])
    }
    pub fn for_each_body_mut<'a>(&mut self, mut f: impl FnMut(&mut Body<'ctxt>) + 'a) {
        for src in self.body_sources.iter() {
            f(self.bodies.get_mut(src).unwrap());
        }
    }
    pub fn add_body(&mut self, body: Body<'ctxt>) {
        let src = body.src;
        self.bodies.insert(src, body);
        self.body_sources.push(src);
    }
    #[track_caller]
    pub fn expect_body(&self, src: BodySource) -> &Body<'ctxt> {
        self.bodies.get(&src).expect("expected a body")
    }
}
