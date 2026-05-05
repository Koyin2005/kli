use crate::{
    ast::{BinaryOp, Ident, Mutable},
    resolved_ast::{Builtin, FunctionId, LocalRegionId, Var, VarId},
    types::{GenericArg, GenericKind, Type},
};
#[derive(Debug)]
pub struct Pattern {
    pub ty: Type,
    pub line: usize,
    pub kind: PatternKind,
}
#[derive(Debug)]
pub enum PatternKind {
    Some(Box<Pattern>),
    None,
    Deref(Box<Pattern>),
    Binding(Mutable, Var, Box<Type>),
}
#[derive(Debug)]
pub struct Place {
    pub ty: Type,
    pub line: usize,
    pub kind: PlaceKind,
}
#[derive(Debug)]
pub enum PlaceKind {
    Var(Var),
    Deref(Box<Expr>),
}
#[derive(Debug)]
pub struct Lambda {
    pub params: Vec<(Ident, VarId, Type)>,
    pub return_type: Type,
    pub body: Expr,
}
#[derive(Debug)]
pub struct Expr {
    pub ty: Type,
    pub line: usize,
    pub kind: ExprKind,
}
#[derive(Debug)]
pub enum ExprKind {
    String(String),
    Bool(bool),
    Int(i64),
    Unit,
    Err,
    None,
    Panic,
    Some(Box<Expr>),
    Builtin(Builtin, Vec<GenericArg>),
    Function(String, FunctionId, Vec<GenericArg>),
    Print(Option<Box<Expr>>),
    List(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Load(Place),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    For {
        pattern: Pattern,
        iterator: Box<Expr>,
        body: Box<Expr>,
    },
    Let {
        pattern: Pattern,
        binder: Box<Expr>,
        body: Box<Expr>,
    },
    Borrow {
        mutable: Mutable,
        var_name: Ident,
        old_var: VarId,
        new_var: VarId,
        region_name: Ident,
        region: LocalRegionId,
        new_ty: Type,
        body: Box<Expr>,
    },
    Case(Box<Expr>, Vec<CaseArm>),
    Sequence(Box<Expr>, Box<Expr>),
    Assign(Place, Box<Expr>),
    Lambda(Box<Lambda>),
}
#[derive(Debug)]
pub struct CaseArm {
    pub pattern: Pattern,
    pub body: Expr,
}
pub struct GenericParam {
    pub name: Ident,
    pub kind: GenericKind,
}
pub struct Param {
    pub name: Ident,
    pub var: VarId,
    pub ty: Type,
}
pub struct Function {
    pub name: Ident,
    pub generics: Vec<GenericParam>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
}

pub struct Program {
    pub functions: Vec<Function>,
}
