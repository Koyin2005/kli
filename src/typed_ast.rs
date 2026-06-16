use std::rc::Rc;

use crate::{
    ast::{BinaryOp, IsResource, Mutable},
    define_id,
    ident::Ident,
    index_vec::IndexVec,
    resolved_ast::{Builtin, FunctionId, LambdaId, LocalRegionId, Var, VarId},
    src_loc::SrcLoc,
    types::{GenericArg, GenericKind, Region, Type},
};

#[derive(Debug)]
pub struct PatternField {
    pub index: FieldId,
    pub name: Ident,
    pub pattern: Pattern,
}
#[derive(Debug)]
pub struct Pattern {
    pub ty: Type,
    pub loc: SrcLoc,
    pub kind: PatternKind,
}
#[derive(Debug)]
pub enum PatternKind {
    Int(i64),
    Bool(bool),
    Some(Box<Pattern>),
    None,
    Ref(Box<Pattern>),
    Binding(Option<Mutable>, Mutable, Var, Box<Type>),
    Record(Vec<PatternField>),
}
#[derive(Debug)]
pub struct Place {
    pub ty: Type,
    pub loc: SrcLoc,
    pub kind: PlaceKind,
}
#[derive(Debug)]
pub enum PlaceKind {
    Upvar(Var),
    Var(Var),
    Deref(Box<Expr>),
}
#[derive(Debug)]
pub struct Lambda {
    pub id: LambdaId,
    pub is_resource: IsResource,
    pub captures: Vec<(Var, Type)>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
}
#[derive(Debug)]
pub struct LetBinding {
    pub pattern: Pattern,
    pub value: Expr,
}
#[derive(Debug)]
pub enum StmtKind {
    Let(LetBinding),
    Expr(Expr),
}
#[derive(Debug)]
pub struct Stmt {
    pub loc: SrcLoc,
    pub kind: StmtKind,
}
#[derive(Debug)]
pub struct BlockBody {
    pub stmts: Vec<Stmt>,
    pub expr: Box<Expr>,
}
#[derive(Debug)]
pub struct Expr {
    pub ty: Type,
    pub loc: SrcLoc,
    pub kind: ExprKind,
}
define_id!(FieldId);

#[derive(Debug)]
pub struct RecordFieldInit {
    pub index: FieldId,
    pub name: Ident,
    pub value: Expr,
}
#[derive(Debug)]
pub enum IteratorType {
    ArrayListRef(Region, Mutable, Type),
    StringIter(Region, Mutable),
}
#[derive(Debug)]
pub enum ExprKind {
    Record(Vec<RecordFieldInit>),
    Block(BlockBody, Option<LocalRegionId>),
    String(Rc<str>),
    Bool(bool),
    Int(i64),
    Unit,
    Err,
    None,
    Panic,
    Some(Box<Expr>),
    Builtin(Builtin, Vec<GenericArg>),
    Function(Rc<str>, FunctionId, Vec<GenericArg>),
    Print(Option<Box<Expr>>),
    List(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Load(Place),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    For {
        pattern: Pattern,
        iterator: Box<Expr>,
        iterator_type: IteratorType,
        body: Box<Expr>,
    },
    Borrow {
        mutable: Mutable,
        place: Place,
        region: Region,
    },
    Case(Box<Expr>, Vec<CaseArm>),
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
#[derive(Debug, Clone)]
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
    pub body: Option<Expr>,
}

pub struct Program {
    pub functions: IndexVec<FunctionId, Function>,
    pub main: FunctionId,
}
