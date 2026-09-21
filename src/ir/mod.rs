use crate::{Symbol, define_id, index_vec::IndexVec, typed_ast::FieldId, types::CaseId};
pub mod lower;
mod print;
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Bool(bool),
    Function(BodyId),
    String(Symbol),
}
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
}
impl Expr {
    pub fn tuple(exprs: impl IntoIterator<Item = Self>) -> Self {
        Self {
            kind: ExprKind::Aggregate(AggregateKind::Tuple, exprs.into_iter().collect()),
        }
    }
    pub fn unit_value() -> Self {
        Self {
            kind: ExprKind::Aggregate(AggregateKind::Tuple, IndexVec::new()),
        }
    }
    pub fn constant(value: Constant) -> Self {
        Self {
            kind: ExprKind::Constant(value),
        }
    }
    pub fn load(place: Place) -> Self {
        Self {
            kind: ExprKind::Load(place),
        }
    }
}
#[derive(Debug, Clone)]
pub enum AggregateKind {
    Tuple,
}
#[derive(Debug, Clone)]
pub enum ExprKind {
    Constant(Constant),
    Load(Place),
    Discriminant(Place),
    Aggregate(AggregateKind, IndexVec<FieldId, Expr>),
}
define_id!(Local);
#[derive(Debug, Clone)]
pub enum Place {
    Local(Local),
    Field(Box<Place>, FieldId),
    Deref(Box<Place>),
    Downcast(Box<Place>, CaseId),
}

#[derive(Debug)]
pub struct SwitchArm {
    pub value: i32,
    pub body: DecisionTree,
}
#[derive(Debug)]
pub struct Switch {
    pub value: Expr,
    pub arms: Vec<SwitchArm>,
    pub otherwise: Box<DecisionTree>,
}
#[derive(Debug)]
pub struct Call {
    pub return_place: Place,
    pub callee: Expr,
    pub args: Vec<Expr>,
}
define_id!(ArmId);
#[derive(Debug)]
pub enum DecisionTree {
    Body(ArmId),
    Switch(Switch),
}
define_id!(LoopLabel);
#[derive(Debug)]
pub struct Loop {
    pub label: LoopLabel,
    pub stmts: Vec<Stmt>,
}
#[derive(Debug)]
pub struct Match {
    pub tree: DecisionTree,
    pub arms: IndexVec<ArmId, Vec<Stmt>>,
}
#[derive(Debug)]
pub enum Stmt {
    Panic,
    Print { value: Expr, is_err: bool },
    Block(Vec<Stmt>),
    Loop(LoopLabel, Vec<Stmt>),
    Break(LoopLabel),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Match(Match),
    Call(Call),
    Assign(Place, Expr),
    Return(Expr),
}
#[derive(Debug)]
pub enum Type {
    Int,
    Bool,
    String,
    Param(u32),
    Function(Vec<Type>, Box<Type>),
    Tuple(Vec<Type>),
    Array(Box<Type>),
}
#[derive(Debug)]
pub struct LocalInfo {
    pub name: Option<Symbol>,
    pub is_mutable: bool,
    pub ty: Type,
}
#[derive(Debug)]
pub struct Body {
    pub name: String,
    pub generic_params: Vec<Symbol>,
    pub param_count: u32,
    pub return_ty: Type,
    pub locals: IndexVec<Local, LocalInfo>,
    pub body: Vec<Stmt>,
}
define_id!(BodyId);
pub struct Program {
    pub bodies: IndexVec<BodyId, Body>,
}
