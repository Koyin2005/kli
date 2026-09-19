use crate::{Symbol, def_ids::DefId, define_id, index_vec::IndexVec, typed_ast::FieldId};

pub enum Constant {
    Int(i64),
    Bool(bool),
    Function(DefId)
}
pub struct Expr{
    pub kind : ExprKind
}
pub enum ExprKind {
    Constant(Constant),
    Load(Place),
    Tuple(Vec<Expr>),
    Discriminant(Place)
}
define_id!(Local);
pub enum Place {
    Local(Local),
    Field(Box<Place>,FieldId)
}

pub struct SwitchArm{
    pub value : i32,
    pub body : Vec<Stmt>
}
pub struct Switch{
    pub value : Expr,
    pub arms : Vec<SwitchArm>,
    pub otherwise : Box<Stmt>
}
pub struct Call{
    pub return_place : Place,
    pub callee : Expr,
    pub args : Vec<Expr>
}
define_id!(LoopLabel);
pub struct Loop{
    pub label : LoopLabel,
    pub stmts : Vec<Stmt>
}
pub enum Stmt {
    Block(Vec<Stmt>),
    Loop(LoopLabel,Vec<Stmt>),
    Break(LoopLabel),
    If(Expr,Vec<Stmt>,Vec<Stmt>),
    Switch(Switch),
    Call(Call),
    Assign(Place,Expr),
    Return(Expr)
}
pub type Type = ();
pub struct LocalInfo{
    pub name : Option<Symbol>,
    pub is_mutable : bool,
    pub ty : Type
}
pub struct Body{
    pub param_count : u32,
    pub return_ty : Type,
    pub locals : IndexVec<Local,LocalInfo>,
    pub body : Vec<Stmt>   
    
}