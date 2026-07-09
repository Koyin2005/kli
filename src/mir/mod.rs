use std::{collections::HashMap, fmt::Display};

use crate::{
    Symbol,
    ast::Mutable,
    collect::CtxtRef,
    def_ids::DefId,
    define_id,
    index_vec::IndexVec,
    resolved_ast::{Var, VarId},
    src_loc::SrcLoc,
    typed_ast::FieldId,
    types::{CaseId, FieldName, GenericArg, GenericArgs, PointerType, Region, Type},
};
pub mod build;
pub mod dump;
pub mod passes;
pub mod visitor;
pub mod well_formed;
define_id!(Local);
impl Local {
    pub const FIRST_PARAM: Self = Self(0);
}
#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub enum PlaceProjection {
    Field(FieldId),
    ConstantIndex(u32),
    Index(Local),
    CaseDowncast(CaseId, Symbol),
    Deref,
}
impl PlaceProjection {
    pub fn apply_projection_to_type(self, ty: Type, ctxt: CtxtRef<'_>) -> Type {
        match self {
            PlaceProjection::Deref => ty
                .into_pointer_type(ctxt)
                .ok()
                .and_then(|(pointer, ty)| match pointer {
                    PointerType::Raw | PointerType::Reference(..) => Some(ty),
                    _ => None,
                })
                .expect("should be a pointer type"),
            PlaceProjection::Field(field) => {
                ty.field_info(field, ctxt)
                    .expect("should be a record type")
                    .0
            }
            PlaceProjection::Index(_) | PlaceProjection::ConstantIndex(_) => {
                let Type::Array(ty, _) = ty else {
                    unreachable!("Should be an array")
                };
                *ty
            }
            PlaceProjection::CaseDowncast(index, _) => {
                let Type::Named(id, _, args) = ty else {
                    unreachable!("Should be named")
                };
                ctxt.type_def(id)
                    .case(index)
                    .expect_field()
                    .type_of(&args, ctxt)
            }
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub enum PlaceBase {
    Local(Local),
    ReturnPlace,
}
impl PlaceBase {
    pub fn type_of(self, locals: &Locals, return_type: &Type) -> Type {
        match self {
            PlaceBase::Local(local) => locals[local].ty.clone(),
            PlaceBase::ReturnPlace => return_type.clone(),
        }
    }
}
impl Display for PlaceBase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(local) => write!(f, "_{}", local.0),
            Self::ReturnPlace => write!(f, "ret"),
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Place {
    pub base: PlaceBase,
    pub projections: Vec<PlaceProjection>,
}
impl Place {
    pub fn local(local: Local) -> Self {
        Self {
            base: PlaceBase::Local(local),
            projections: Vec::new(),
        }
    }
    pub fn return_place() -> Self {
        Self {
            base: PlaceBase::ReturnPlace,
            projections: Vec::new(),
        }
    }
    pub fn with_field(mut self, field: FieldId) -> Self {
        self.projections.push(PlaceProjection::Field(field));
        self
    }
    pub fn with_index(mut self, index: Local) -> Self {
        self.projections.push(PlaceProjection::Index(index));
        self
    }
    pub fn with_constant_index(mut self, index: u32) -> Self {
        self.projections.push(PlaceProjection::ConstantIndex(index));
        self
    }
    pub fn with_deref(mut self) -> Self {
        self.projections.push(PlaceProjection::Deref);
        self
    }
    pub fn with_case_downcast(mut self, index: CaseId, name: Symbol) -> Self {
        self.projections
            .push(PlaceProjection::CaseDowncast(index, name));
        self
    }

    pub fn type_of(&self, ctxt: CtxtRef<'_>, locals: &Locals, return_type: &Type) -> Type {
        let mut ty = self.base.type_of(locals, return_type);
        for projection in self.projections.iter() {
            ty = projection.apply_projection_to_type(ty, ctxt);
        }
        ty
    }
}
#[derive(Clone, Debug)]
pub struct Constant {
    pub ty: Box<Type>,

    pub value: ConstantValue,
}
impl Constant {
    pub fn bool(value: bool) -> Self {
        Self {
            ty: Box::new(Type::Bool),
            value: ConstantValue::Bool(value),
        }
    }
    pub fn byte(value: u8) -> Self {
        Self {
            ty: Box::new(Type::Byte),
            value: ConstantValue::Int(value as i64),
        }
    }
    pub fn int(value: i64) -> Self {
        Self {
            ty: Box::new(Type::Int),
            value: ConstantValue::Int(value),
        }
    }
    pub fn zero_sized(ty: Type) -> Self {
        Self {
            ty: Box::new(ty),
            value: ConstantValue::ZeroSized,
        }
    }
    pub fn unit() -> Self {
        Self {
            ty: Box::new(Type::Unit),
            value: ConstantValue::ZeroSized,
        }
    }
}
#[derive(Clone, Debug)]
pub enum ConstantValue {
    Int(i64),
    Bool(bool),
    NamedConst(DefId, Vec<GenericArg>),
    ClosureShim(DefId, Vec<GenericArg>),
    ZeroSized,
}
impl ConstantValue {
    pub const MAX_INT: i64 = i64::MAX;
    pub const MIN_INT: i64 = i64::MIN;

    pub fn as_scalar(&self) -> Option<i128> {
        match *self {
            Self::Bool(value) => Some(value as i128),
            Self::Int(value) => Some(value as i128),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Operand {
    Load(Place),
    Constant(Constant),
}
impl Operand {
    pub fn type_of(&self, ctxt: CtxtRef<'_>, locals: &Locals, return_type: &Type) -> Type {
        match self {
            Operand::Constant(constant) => (*constant.ty).clone(),
            Operand::Load(place) => place.type_of(ctxt, locals, return_type),
        }
    }
}
#[derive(Clone, Debug)]
pub enum AggregateKind {
    Record {
        field_names: IndexVec<FieldId, FieldName>,
    },
    Closure(Vec<Type>, Box<Type>),
    NamedRecord(DefId, GenericArgs),
    Variant(DefId, CaseId, GenericArgs),
    Array(Type, u64),
    String,
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
    Unchecked(OverflowOp),
    Wrapping(OverflowOp),
    Greater,
    Offset,
    Divide,
    Equals,
    BitwiseAnd,
    Lesser,
}
#[derive(Clone, Debug)]
pub enum PointerCast {
    RawToRaw(Type),
}
#[derive(Clone, Debug)]
pub enum CastKind {
    Transmute(Type),
    PointerCast(PointerCast),
}
#[derive(Clone, Debug)]
pub enum Rvalue {
    Aggregate(AggregateKind, IndexVec<FieldId, Operand>),
    Use(Operand),
    Call(Operand, Vec<Operand>),
    Binary(BinaryOp, Box<(Operand, Operand)>),
    Ref(Mutable, Region, Place),
    RawPtrTo(Place),
    Allocate { ty: Type, count: Operand },
    Cast(CastKind, Operand),
    DecodeUtf8(Operand, Operand),
    Len(Place),
    Discriminant(Place),
    DanglingPtr(Type),
}
impl Rvalue {
    pub fn can_remove_if_unused(&self) -> bool {
        match self {
            Self::Aggregate(..)
            | Self::Binary(..)
            | Self::Cast(..)
            | Self::Use(_)
            | Self::Ref(..)
            | Self::RawPtrTo(_)
            | Self::Len(_)
            | Self::DanglingPtr(_)
            | Self::Discriminant(_) => true,
            Self::Allocate { .. } | Self::Call(..) | Self::DecodeUtf8(..) => false,
        }
    }
    pub fn pointer_cast(cast: PointerCast, operand: Operand) -> Self {
        Self::Cast(CastKind::PointerCast(cast), operand)
    }

    pub fn type_of(&self, ctxt: CtxtRef<'_>, locals: &Locals, return_type: &Type) -> Type {
        match self {
            Rvalue::Use(operand) => operand.type_of(ctxt, locals, return_type),
            Rvalue::Len(_) => Type::Int,
            &Rvalue::Ref(mutable, region, ref place) => place
                .type_of(ctxt, locals, return_type)
                .reference(mutable, region),
            Rvalue::Call(operand, _) => {
                let Type::Function(function) = operand.type_of(ctxt, locals, return_type) else {
                    unreachable!("Should be a function type")
                };
                *function.return_type
            }
            Rvalue::Binary(op, left_and_right) => match op {
                BinaryOp::Overflow(_) => Type::pair(Type::Bool, Type::Int),
                BinaryOp::Unchecked(_) | BinaryOp::Wrapping(_) => Type::Int,
                BinaryOp::Offset => {
                    let (left, _) = left_and_right.as_ref();
                    let (PointerType::Raw, ty) = left
                        .type_of(ctxt, locals, return_type)
                        .into_pointer_type(ctxt)
                        .unwrap()
                    else {
                        unreachable!("should be a raw pointer")
                    };
                    Type::pointer(ty)
                }
                BinaryOp::Divide | BinaryOp::BitwiseAnd => Type::Int,
                BinaryOp::Equals => Type::Bool,
                BinaryOp::Lesser | BinaryOp::Greater => Type::Bool,
            },
            Rvalue::Allocate { ty, count: _ } => Type::pointer(ty.clone()),
            Rvalue::DecodeUtf8(_, _) => Type::pair(Type::Char, Type::Int),
            Rvalue::Aggregate(aggregate, operands) => match aggregate {
                AggregateKind::Array(ty, count) => Type::Array(Box::new(ty.clone()), *count),
                AggregateKind::Record { field_names } => Type::Record(
                    field_names
                        .iter()
                        .zip(operands)
                        .map(|(&name, operand)| crate::types::RecordField {
                            name,
                            ty: operand.type_of(ctxt, locals, return_type),
                        })
                        .collect(),
                ),
                AggregateKind::Closure(params, return_type) => Type::function_type(
                    crate::ast::IsResource::Resource,
                    params.clone(),
                    (**return_type).clone(),
                ),
                &AggregateKind::Variant(id, _, ref args)
                | &AggregateKind::NamedRecord(id, ref args) => {
                    let name = ctxt.type_def(id).name;
                    Type::Named(id, name, args.clone())
                }
                AggregateKind::String => Type::String,
            },
            Rvalue::Cast(cast, _) => match cast {
                CastKind::PointerCast(cast) => match cast {
                    PointerCast::RawToRaw(to) => Type::pointer(to.clone()),
                },
                CastKind::Transmute(ty) => ty.clone(),
            },
            Rvalue::Discriminant(_) => Type::Int,
            Rvalue::RawPtrTo(place) => Type::pointer(place.type_of(ctxt, locals, return_type)),
            Rvalue::DanglingPtr(ty) => Type::pointer(ty.clone()),
        }
    }
}
#[derive(Clone)]
pub struct SwitchTarget {
    pub value: i128,
    pub target: BasicBlockId,
}
#[derive(Clone)]
pub struct SwitchTargets {
    pub targets: Vec<SwitchTarget>,
    pub otherwise: BasicBlockId,
}
impl SwitchTargets {
    pub fn branch_for_value(&self, value: i128) -> BasicBlockId {
        self.targets
            .iter()
            .find_map(|target| (target.value == value).then_some(target.target))
            .unwrap_or(self.otherwise)
    }
}
#[derive(Clone)]
pub enum AssertKind {
    Overflow(OverflowOp),
    DivideOverflow,
    DivideByZero,
}
#[derive(Clone)]
pub struct Terminator {
    pub src_info: SrcLoc,
    pub kind: TerminatorKind,
}
impl Terminator {
    pub fn successors(&self) -> impl Iterator<Item = BasicBlockId> {
        let (single, multiple) = match &self.kind {
            TerminatorKind::Goto(block) | TerminatorKind::Assert(.., block) => (Some(*block), None),
            TerminatorKind::Switch(_, switch_targets) => (
                None,
                Some(
                    switch_targets
                        .targets
                        .iter()
                        .map(|target| target.target)
                        .chain(std::iter::once(switch_targets.otherwise)),
                ),
            ),
            TerminatorKind::Unreachable => None.unzip(),
            TerminatorKind::Return => None.unzip(),
            TerminatorKind::Panic => None.unzip(),
        };
        single.into_iter().chain(multiple.into_iter().flatten())
    }
    pub fn successors_mut(&mut self) -> impl Iterator<Item = &mut BasicBlockId> {
        let (single, multiple) = match &mut self.kind {
            TerminatorKind::Goto(block) | TerminatorKind::Assert(..,block) => (Some(block), None),
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
            TerminatorKind::Return => None.unzip(),
            TerminatorKind::Panic => None.unzip(),
        };
        single.into_iter().chain(multiple.into_iter().flatten())
    }
}
#[derive(Clone)]
pub enum TerminatorKind {
    Assert(Operand,AssertKind,BasicBlockId),
    Switch(Operand, SwitchTargets),
    Unreachable,
    Return,
    Goto(BasicBlockId),
    Panic,
}

#[derive(Clone, Copy)]
pub struct Location {
    pub block: BasicBlockId,
    pub stmt: Option<StmtId>,
}

#[derive(Clone)]
pub struct Stmt {
    pub loc: SrcLoc,
    pub kind: StmtKind,
}
#[derive(Clone)]
pub struct CopyNonOverlapping {
    pub dst: Operand,
    pub src: Operand,
    pub count: Operand,
}
#[derive(Clone)]
pub struct DropInPlace {
    pub pointer_to_place: Operand,
}
#[derive(Clone)]
pub enum StmtKind {
    Noop,
    Assign(Place, Box<Rvalue>),
    Print(Option<Operand>),
    Deallocate(Operand),
    CopyNonOverlapping(Box<CopyNonOverlapping>),
    DropInPlace(Box<DropInPlace>),
}
define_id!(BasicBlockId);
impl BasicBlockId {
    pub const ENTRY: Self = Self(0);
}
define_id!(StmtId);
#[derive(Default, Clone)]
pub struct BasicBlock {
    pub stmts: IndexVec<StmtId, Stmt>,
    pub terminator: Option<Terminator>,
}
impl BasicBlock {
    #[track_caller]
    pub fn expect_terminator(&self) -> &Terminator {
        self.terminator
            .as_ref()
            .expect("Block should have a terminator")
    }
    #[track_caller]
    pub fn expect_terminator_mut(&mut self) -> &mut Terminator {
        self.terminator
            .as_mut()
            .expect("Block should have a terminator")
    }
}
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum BodySource {
    Function(DefId),
    Lambda(DefId),
    ClosureShim(DefId),
}
impl BodySource {
    fn def_id(self) -> DefId {
        match self {
            Self::ClosureShim(id) => id,
            Self::Function(id) => id,
            Self::Lambda(id) => id,
        }
    }
    pub fn is_child_of(self, name: Symbol, ctxt: CtxtRef) -> bool {
        ctxt.self_with_anecstors(self.def_id())
            .any(|id| ctxt.ident(id).map(|ident| ident.symbol) == Some(name))
    }
}
#[derive(Clone)]
pub enum LocalKind {
    Temp,
    Env,
    Var(Var),
    Param(Option<Var>),
}
#[derive(Clone)]
pub struct LocalInfo {
    pub ty: Type,
    pub kind: LocalKind,
}
#[derive(Clone)]
pub struct Body {
    pub src: BodySource,
    pub return_type: Type,
    pub locals: Locals,
    pub blocks: IndexVec<BasicBlockId, BasicBlock>,
}
impl Body {
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
            Some(stmt) => self.blocks[loc.block].stmts[stmt].loc,
            None => self.blocks[loc.block].expect_terminator().src_info,
        }
    }
}
pub type Locals = IndexVec<Local, LocalInfo>;

#[derive(Default)]
pub struct Context {
    pub check_well_formed: bool,
    pub(super) bodies: HashMap<BodySource, Body>,
    pub(super) body_sources: Vec<BodySource>,
}
impl Context {
    pub fn new(well_formed: bool) -> Self {
        Self {
            check_well_formed: well_formed,
            ..Default::default()
        }
    }
    pub fn body_iter(&self) -> impl Iterator<Item = &Body> {
        self.body_sources.iter().map(|src| &self.bodies[src])
    }
    pub fn for_each_body_mut(&mut self, mut f: impl FnMut(&mut Body)) {
        for src in self.body_sources.iter() {
            f(self.bodies.get_mut(src).unwrap());
        }
    }
    #[track_caller]
    pub fn expect_body(&self, src: BodySource) -> &Body {
        self.bodies.get(&src).expect("expected a body")
    }
}
