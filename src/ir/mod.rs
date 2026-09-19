use crate::{
    Symbol, def_ids::DefId, define_id, index_vec::IndexVec, typed_ast::FieldId, types::CaseId,
};

pub enum Constant {
    Int(i64),
    Bool(bool),
    Function(DefId),
}
pub struct Expr {
    pub kind: ExprKind,
}
pub enum AggregateKind {
    Tuple,
}
pub enum ExprKind {
    Constant(Constant),
    Load(Place),
    Tuple(Vec<Expr>),
    Discriminant(Place),
    Aggregate(AggregateKind, IndexVec<FieldId, Expr>),
}
define_id!(Local);
pub enum Place {
    Local(Local),
    Field(Box<Place>, FieldId),
    Deref(Box<Place>),
    Downcast(Box<Place>, CaseId),
}

pub struct SwitchArm {
    pub value: i32,
    pub body: DecisionTree,
}
pub struct Switch {
    pub value: Expr,
    pub arms: Vec<SwitchArm>,
    pub otherwise: Box<DecisionTree>,
}
pub struct Call {
    pub return_place: Place,
    pub callee: Expr,
    pub args: Vec<Expr>,
}
define_id!(ArmId);
pub enum DecisionTree {
    Body(ArmId),
    Switch(Switch),
}
define_id!(LoopLabel);
pub struct Loop {
    pub label: LoopLabel,
    pub stmts: Vec<Stmt>,
}
pub struct Match{
    pub tree : DecisionTree,
    pub arms : IndexVec<ArmId,Vec<Stmt>>
}
pub enum Stmt {
    Block(Vec<Stmt>),
    Loop(LoopLabel, Vec<Stmt>),
    Break(LoopLabel),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Match(Match),
    Call(Call),
    Assign(Place, Expr),
    Return(Expr),
}
pub type Type = ();
pub struct LocalInfo {
    pub name: Option<Symbol>,
    pub is_mutable: bool,
    pub ty: Type,
}
pub struct Body {
    pub param_count: u32,
    pub return_ty: Type,
    pub locals: IndexVec<Local, LocalInfo>,
    pub body: Vec<Stmt>,
}
