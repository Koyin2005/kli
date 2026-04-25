use crate::{ast::Ident, types::{GenericKind, Type}};

#[derive(Debug)]
pub struct Expr{
    pub ty : Type,
    pub line : usize,
    pub kind : ExprKind
}
#[derive(Debug)]
pub enum ExprKind {
    String(String),
    Bool(bool),
    Int(i64),
    List(Vec<Expr>),
    Call(Box<Expr>,Vec<Expr>),
    Variable(String,usize),
    
}

pub struct GenericParam{
    pub name : Ident,
    pub kind : GenericKind
}
pub struct Function{
    pub generics : Vec<GenericParam>,
    pub body : Expr
}