use std::{collections::HashMap, fmt::Display, rc::Rc};

use crate::{
    ast::Mutable,
    define_id,
    ident::Ident,
    index_vec::IndexVec,
    resolved_ast::{Builtin, FunctionId, LambdaId, Var, VarId},
    src_loc::SrcLoc,
    typed_ast::FieldId,
    types::{GenericArg, PointerType, Region, Type},
};
pub mod build;
pub mod dump;
pub mod visitor;
pub mod well_formed;
define_id!(Local);
#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub enum PlaceProjection {
    DowncastSome,
    Field(FieldId),
    ConstantIndex(u32),
    Index(Local),
    Deref,
}
#[derive(Clone, PartialEq, Eq, Hash, Debug, Copy)]
pub enum PlaceBase {
    Local(Local),
    ReturnPlace,
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
        Self {
            base: self.base,
            projections: self.projections,
        }
    }
    pub fn with_index(mut self, index: Local) -> Self {
        self.projections.push(PlaceProjection::Index(index));
        Self {
            base: self.base,
            projections: self.projections,
        }
    }
    pub fn with_downcast_some(mut self) -> Self {
        self.projections.push(PlaceProjection::DowncastSome);
        Self {
            base: self.base,
            projections: self.projections,
        }
    }
    pub fn with_constant_index(mut self, index: u32) -> Self {
        self.projections.push(PlaceProjection::ConstantIndex(index));
        Self {
            base: self.base,
            projections: self.projections,
        }
    }
    pub fn with_deref(mut self) -> Self {
        self.projections.push(PlaceProjection::Deref);
        Self {
            base: self.base,
            projections: self.projections,
        }
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
    Builtin(Builtin, Vec<GenericArg>),
    Function(FunctionId, Vec<GenericArg>),
    Lambda(LambdaId, Vec<GenericArg>),
    ZeroSized,
}
impl ConstantValue {
    pub const MAX_INT: i64 = i64::MAX;
    pub const MIN_INT: i64 = i64::MIN;
}
#[derive(Clone, Debug)]
pub enum Operand {
    Load(Place),
    Constant(Constant),
}
#[derive(Clone, Debug)]
pub enum AggregateKind {
    Record {
        field_names: IndexVec<FieldId, Rc<str>>,
    },
    Closure(Vec<Type>, Box<Type>),
    Option {
        inner: Type,
        is_some: bool,
    },
    ArrayList(Type),
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
    Offset,
    Divide,
    Equals,
    BitwiseAnd,
    Lesser,
}
#[derive(Clone, Debug)]
pub enum PointerCast {
    RawToRaw(Type),
    BoxToRaw,
    RawToBox,
    RefToRaw(Mutable),
    RawToRef(Mutable, Region),
    Freeze,
}
#[derive(Clone, Debug)]
pub enum Rvalue {
    Aggregate(AggregateKind, IndexVec<FieldId, Operand>),
    Use(Operand),
    Call(Operand, Vec<Operand>),
    Binary(BinaryOp, Box<(Operand, Operand)>),
    Ref(Mutable, Region, Place),
    Allocate { ty: Type, count: Operand },
    PointerCast(PointerCast, Operand),
    DecodeUtf8(Operand, Operand),
    Len(Place),
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
#[derive(Clone)]
pub enum TerminatorKind {
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
pub enum StmtKind {
    Noop,
    Assign(Place, Box<Rvalue>),
    Assert(Operand, AssertKind),
    Print(Option<Operand>),
    Deallocate(Operand),
}
define_id!(BasicBlockId);
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
    Function(FunctionId),
    Lambda(LambdaId),
}
#[derive(Clone)]
pub enum LocalKind {
    Temp,
    Env,
    Var(Var),
    Param(Var),
}
#[derive(Clone)]
pub struct LocalInfo {
    pub ty: Type,
    pub kind: LocalKind,
}

#[derive(Clone)]
pub struct Captures {
    ///The local for the restored pointer with the proper type
    pub env_ptr: Option<Local>,
    pub captures: Vec<(Var, Type)>,
}
impl Captures {
    pub fn env_type(&self) -> Type {
        Type::record(self.captures.iter().map(|(_, ty)| ty.clone()).collect())
    }
}
#[derive(Clone)]
pub struct Body {
    pub src: BodySource,
    pub return_type: Type,
    pub capture_info: Option<Captures>,
    pub locals: Locals,
    pub blocks: IndexVec<BasicBlockId, BasicBlock>,
}
impl Body {
    pub fn local_for_var(&self, var_id: VarId) -> Option<Local> {
        self.locals
            .iter()
            .position(|local| {
                let (LocalKind::Var(var) | LocalKind::Param(var)) = &local.kind else {
                    return false;
                };
                var.1 == var_id
            })
            .map(Local::new)
    }
    pub fn type_of_base(&self, base: &PlaceBase) -> Type {
        match base {
            PlaceBase::Local(local) => self.locals[*local].ty.clone(),
            PlaceBase::ReturnPlace => self.return_type.clone(),
        }
    }
    pub fn apply_projection_to_type(&self, ty: Type, projection: &PlaceProjection) -> Type {
        match projection {
            PlaceProjection::Deref => ty
                .as_pointer_type()
                .ok()
                .and_then(|(pointer, ty)| match pointer {
                    PointerType::Raw | PointerType::Reference(..) => Some(ty),
                    _ => None,
                })
                .expect("should be a pointer type"),
            &PlaceProjection::Field(field) => {
                ty.field_type(field).expect("should be a record type")
            }
            PlaceProjection::Index(_) | PlaceProjection::ConstantIndex(_) => {
                let Type::Array(ty, _) = ty else {
                    unreachable!("Should be an array")
                };
                *ty
            }
            PlaceProjection::DowncastSome => {
                ty.as_option().expect("should be an option type").clone()
            }
        }
    }
    pub fn type_of_place(&self, place: &Place) -> Type {
        let mut ty = self.type_of_base(&place.base);
        for projection in place.projections.iter() {
            ty = self.apply_projection_to_type(ty, projection);
        }
        ty
    }
    pub fn type_of_operand(&self, operand: &Operand) -> Type {
        match operand {
            Operand::Constant(constant) => (*constant.ty).clone(),
            Operand::Load(place) => self.type_of_place(place),
        }
    }
    pub fn type_of_rvalue(&self, rvalue: &Rvalue) -> Type {
        match rvalue {
            Rvalue::Use(operand) => self.type_of_operand(operand),
            Rvalue::Len(_) => Type::Int,
            Rvalue::Ref(mutable, region, place) => self
                .type_of_place(place)
                .reference(*mutable, region.clone()),
            Rvalue::Call(operand, _) => {
                let Type::Function(function) = self.type_of_operand(operand) else {
                    unreachable!("Should be a function type")
                };
                *function.return_type
            }
            Rvalue::Binary(op, left_and_right) => match op {
                BinaryOp::Overflow(_) => Type::record([Type::Bool, Type::Int].into()),
                BinaryOp::Unchecked(_) | BinaryOp::Wrapping(_) => Type::Int,
                BinaryOp::Offset => {
                    let (left, _) = left_and_right.as_ref();
                    let (PointerType::Raw, ty) =
                        self.type_of_operand(left).as_pointer_type().unwrap()
                    else {
                        unreachable!("should be a raw pointer")
                    };
                    Type::pointer(ty)
                }
                BinaryOp::Divide | BinaryOp::BitwiseAnd => Type::Int,
                BinaryOp::Equals => Type::Bool,
                BinaryOp::Lesser => Type::Bool,
            },
            Rvalue::Allocate { ty, count: _ } => Type::pointer(ty.clone()),
            Rvalue::DecodeUtf8(_, _) => Type::record([Type::Char, Type::Int].into()),
            Rvalue::Aggregate(aggregate, operands) => match aggregate {
                AggregateKind::Array(ty, count) => Type::Array(Box::new(ty.clone()), *count),
                AggregateKind::Record { field_names } => Type::Record(
                    field_names
                        .iter()
                        .zip(operands)
                        .map(|(name, operand)| crate::types::RecordField {
                            name: name.clone(),
                            ty: self.type_of_operand(operand),
                        })
                        .collect(),
                ),
                AggregateKind::Closure(params, return_type) => Type::function_type(
                    crate::ast::IsResource::Resource,
                    params.clone(),
                    (**return_type).clone(),
                ),
                AggregateKind::Option { inner, .. } => Type::Option(Box::new(inner.clone())),
                AggregateKind::ArrayList(ty) => Type::List(Box::new(ty.clone())),
                AggregateKind::String => Type::String,
            },
            Rvalue::PointerCast(cast, operand) => {
                let (pointer_type, pointee) = self
                    .type_of_operand(operand)
                    .as_pointer_type()
                    .expect("should be a pointer type");
                match cast {
                    PointerCast::Freeze => {
                        let PointerType::Reference(region, Mutable::Mutable) = pointer_type else {
                            unreachable!("should be a reference")
                        };
                        Type::Imm(region, Box::new(pointee))
                    }
                    PointerCast::BoxToRaw | PointerCast::RefToRaw(_) => Type::pointer(pointee),
                    PointerCast::RawToRaw(ty) => Type::pointer(ty.clone()),
                    PointerCast::RawToBox => Type::Box(Box::new(pointee)),
                    PointerCast::RawToRef(mutable, region) => {
                        Type::reference(pointee, *mutable, region.clone())
                    }
                }
            }
        }
    }
    pub fn src_info(&self, loc: Location) -> SrcLoc {
        match loc.stmt {
            Some(stmt) => self.blocks[loc.block].stmts[stmt].loc.clone(),
            None => self.blocks[loc.block].expect_terminator().src_info.clone(),
        }
    }
}
pub type Locals = IndexVec<Local, LocalInfo>;

#[derive(Default)]
pub struct Context {
    pub function_names: IndexVec<FunctionId, Ident>,
    pub(super) bodies: HashMap<BodySource, Body>,
    pub(super) body_sources: Vec<BodySource>,
}
impl Context {
    pub fn body_iter(&self) -> impl Iterator<Item = &Body> {
        self.body_sources.iter().map(|src| &self.bodies[src])
    }
    #[track_caller]
    pub fn expect_body(&self, src: BodySource) -> &Body {
        self.bodies.get(&src).expect("expected a body")
    }
}
