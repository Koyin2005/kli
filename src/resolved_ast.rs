use std::rc::Rc;

use crate::{
    ast::{BinaryOp, IsResource, Mutable},
    ident::Ident,
    src_loc::SrcLoc,
};

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub struct FunctionId(usize);
impl FunctionId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }
}
impl From<FunctionId> for usize {
    fn from(value: FunctionId) -> Self {
        value.0
    }
}

#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub struct VarId(usize);
impl VarId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }
}
impl From<VarId> for usize {
    fn from(value: VarId) -> Self {
        value.0
    }
}
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub struct LocalRegionId(usize);
impl LocalRegionId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }
}
impl From<LocalRegionId> for usize {
    fn from(value: LocalRegionId) -> Self {
        value.0
    }
}
#[derive(Debug, Clone)]
pub struct Var(pub Rc<str>, pub VarId);
#[derive(Debug)]
pub struct BorrowExpr {
    pub mutable: Mutable,
    pub var_name: Ident,
    pub old_var: VarId,
    pub new_var: VarId,
    pub region_name: Ident,
    pub region: LocalRegionId,
    pub body: Expr,
}
#[derive(Debug)]
pub struct Lambda {
    pub params: Vec<(Ident, VarId, Option<Type>)>,
    pub resource: IsResource,
    pub body: Expr,
}
#[derive(Debug)]
pub struct Place {
    pub loc: SrcLoc,
    pub kind: PlaceKind,
}
#[derive(Debug)]
pub enum PlaceKind {
    Var(Var),
    Deref(Box<Expr>),
}

#[derive(Clone, Copy, Debug)]
pub enum Builtin {
    AllocBox,
    DeallocBox,
    DestroyList,
    DerefBox,
    DerefBoxMut,
    Freeze,
    DestroyString,
    Replace,
    Swap,
}
#[derive(Debug)]
pub struct LetBinding {
    pub pattern: Pattern,
    pub ty: Option<Type>,
    pub value: Expr,
}
#[derive(Debug)]
pub struct CaseArm {
    pub pattern: Pattern,
    pub body: Expr,
}
#[derive(Debug)]
pub enum StmtKind {
    Let(Box<LetBinding>),
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
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
}
#[derive(Debug)]
pub enum ExprKind {
    Block(BlockBody),
    Unit,
    Err,
    Annotate(Box<Expr>, Box<Type>),
    Int(i64),
    Bool(bool),
    String(Rc<str>),
    Var(Rc<str>, VarId),
    Function(Rc<str>, FunctionId),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Borrow(Box<BorrowExpr>),
    Some(Box<Expr>),
    None(Option<Type>),
    Panic(Option<Type>),
    Lambda(Box<Lambda>),
    Deref(Box<Expr>),
    Assign(Place, Box<Expr>),
    For(Pattern, Box<Expr>, Box<Expr>),
    Builtin(Builtin),
    Case(Box<Expr>, Vec<CaseArm>),
    Print(Option<Box<Expr>>),
    List(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Record(Vec<FieldInit>),
}
#[derive(Debug, Clone)]
pub enum RegionKind {
    Param(Rc<str>, usize),
    Local(Rc<str>, LocalRegionId),
    Static,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub loc: SrcLoc,
    pub kind: RegionKind,
}

#[derive(Debug)]
pub struct PatternField {
    pub name: Ident,
    pub pattern: Pattern,
}
#[derive(Debug)]
pub enum PatternKind {
    Bool(bool),
    Some(Box<Pattern>),
    None,
    Binding(Mutable, Ident, VarId),
    Record(Vec<PatternField>),
}
#[derive(Debug)]
pub struct Pattern {
    pub loc: SrcLoc,
    pub kind: PatternKind,
}
#[derive(Debug)]
pub struct Expr {
    pub loc: SrcLoc,
    pub kind: ExprKind,
}
#[derive(Debug)]
pub struct Param {
    pub loc: SrcLoc,
    pub var: Var,
    pub ty: Type,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericKind {
    Region,
    Type,
}
#[derive(Debug)]
pub struct Generics {
    pub loc: SrcLoc,
    pub names: Vec<Ident>,
    pub kinds: Vec<GenericKind>,
}
#[derive(Debug)]
pub struct Function {
    pub loc: SrcLoc,
    pub name: Ident,
    pub generics: Option<Generics>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
}
#[derive(Debug)]
pub struct RecordFieldType {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub enum TypeKind {
    Unit,
    Int,
    Bool,
    String,
    Char,
    List(Box<Type>),
    Box(Box<Type>),
    Option(Box<Type>),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
    Function(IsResource, Vec<Type>, Box<Type>),
    Param(Rc<str>, usize),
    Unknown,
    Record(Vec<RecordFieldType>),
}
#[derive(Debug)]
pub struct Type {
    pub loc: SrcLoc,
    pub kind: TypeKind,
}
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}
