use std::{collections::HashMap, fmt::Display, rc::Rc};

use crate::{
    ast::Mutable,
    define_id,
    ident::Ident,
    index_vec::IndexVec,
    resolved_ast::{FunctionId, Var, VarId},
    typed_ast::{FieldId, LambdaId},
    types::{GenericArg, Type},
};
pub mod build;
pub mod dump;
define_id!(Local);
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum PlaceProjection {
    Field(FieldId),
    ConstantIndex(u32),
    Index(Local),
    Len,
    Deref,
}
#[derive(Clone, PartialEq, Eq, Hash)]
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
#[derive(Clone, PartialEq, Eq, Hash)]
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
    pub fn with_constant_index(mut self, index: u32) -> Self {
        self.projections.push(PlaceProjection::ConstantIndex(index));
        Self {
            base: self.base,
            projections: self.projections,
        }
    }
    pub fn with_len(mut self) -> Self {
        self.projections.push(PlaceProjection::Len);
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
#[derive(Clone)]
pub struct Constant {
    pub ty: Type,

    pub value: ConstantValue,
}
impl Constant {
    pub const fn bool(value: bool) -> Self {
        Self {
            ty: Type::Bool,
            value: ConstantValue::Bool(value),
        }
    }
    pub const fn int(value: i64) -> Self {
        Self {
            ty: Type::Int,
            value: ConstantValue::Int(value),
        }
    }
    pub const fn zero_sized(ty: Type) -> Self {
        Self {
            ty,
            value: ConstantValue::ZeroSized,
        }
    }
    pub const fn unit() -> Self {
        Self {
            ty: Type::Unit,
            value: ConstantValue::ZeroSized,
        }
    }
}
#[derive(Clone)]
pub enum ConstantValue {
    Int(i64),
    Bool(bool),
    Function(FunctionId, Vec<GenericArg>),
    Lambda(LambdaId, Vec<GenericArg>),
    ZeroSized,
}
impl ConstantValue {
    pub const MAX_INT: i64 = i64::MAX;
    pub const MIN_INT: i64 = i64::MIN;
}
#[derive(Clone)]
pub enum Operand {
    Load(Place),
    Constant(Constant),
}
pub enum AggregateKind {
    Record {
        field_names: IndexVec<FieldId, Rc<str>>,
    },
    Closure,
}
#[derive(Debug, Clone, Copy)]
pub enum OverflowOp {
    Add,
    Subtract,
    Multiply,
}
#[derive(Debug)]
pub enum BinaryOp {
    Overflow(OverflowOp),
    Unchecked(OverflowOp),
    Divide,
    Equals,
    BitwiseAnd,
    Lesser,
}
pub enum Rvalue {
    Aggregate(AggregateKind, IndexVec<FieldId, Operand>),
    Use(Operand),
    Call(Operand, Vec<Operand>),
    Binary(BinaryOp, Box<(Operand, Operand)>),
    Ref(Mutable, Place),
    AllocateArray(Type, Operand),
    AllocateEnv(Vec<(Var, Operand)>),
}
pub struct SwitchTarget {
    pub value: i128,
    pub target: BasicBlockId,
}
pub struct SwitchTargets {
    pub targets: Vec<SwitchTarget>,
    pub otherwise: BasicBlockId,
}
pub enum AssertKind {
    Overflow(OverflowOp),
    DivideOverflow,
    DivideByZero,
}
pub enum Terminator {
    Switch(Operand, SwitchTargets),
    Unreachable,
    Return,
    Goto(BasicBlockId),
    Panic,
}
pub enum Stmt {
    Noop,
    Assign(Place, Rvalue),
    Assert(Operand, AssertKind),
    Print(Option<Operand>),
}
define_id!(BasicBlockId);
define_id!(StmtId);
#[derive(Default)]
pub struct BasicBlock {
    pub stmts: IndexVec<StmtId, Stmt>,
    pub terminator: Option<Terminator>,
}
impl BasicBlock {
    #[track_caller]
    pub fn expect_terminator(&self) -> &Terminator {
        self.terminator.as_ref().unwrap()
    }
    #[track_caller]
    pub fn expect_terminator_mut(&mut self) -> &mut Terminator {
        self.terminator.as_mut().unwrap()
    }
}
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub enum BodySource {
    Function(FunctionId),
    Lambda(LambdaId),
}
pub enum LocalKind {
    Temp,
    Env,
    Var(Var),
    Param(Var),
}
pub struct LocalInfo {
    pub ty: Type,
    pub kind: LocalKind,
}
pub struct Body {
    pub src: BodySource,
    pub return_type: Type,
    pub captures: Vec<(Var, Type)>,
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
}
#[derive(Default)]
pub struct Context {
    pub function_names: IndexVec<FunctionId, Ident>,
    pub bodies: HashMap<BodySource, Body>,
    pub body_sources: Vec<BodySource>,
}
pub type Locals = IndexVec<Local, LocalInfo>;
