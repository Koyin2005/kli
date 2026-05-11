use crate::ast::{BinaryOp, Ident, IsResource, Mutable};

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
pub struct Var(pub String, pub VarId);
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
    pub line: usize,
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
pub struct LetExpr {
    pub pattern: Pattern,
    pub ty: Option<Type>,
    pub binder: Expr,
    pub body: Expr,
}
#[derive(Debug)]
pub struct CaseArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug)]
pub enum ExprKind {
    Unit,
    Err,
    Annotate(Box<Expr>, Box<Type>),
    Int(i64),
    Bool(bool),
    String(String),
    Var(String, VarId),
    Function(String, FunctionId),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Let(Box<LetExpr>),
    Borrow(Box<BorrowExpr>),
    Some(Box<Expr>),
    None(Option<Type>),
    Panic(Option<Type>),
    Lambda(Box<Lambda>),
    Deref(Box<Expr>),
    Assign(Place, Box<Expr>),
    For(Pattern, Box<Expr>, Box<Expr>),
    Sequence(Box<Expr>, Box<Expr>),
    Builtin(Builtin),
    Case(Box<Expr>, Vec<CaseArm>),
    Print(Option<Box<Expr>>),
    List(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>)
}
#[derive(Debug, Clone)]
pub enum RegionKind {
    Param(String, usize),
    Local(String, LocalRegionId),
    Static,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Region {
    pub line: usize,
    pub kind: RegionKind,
}

#[derive(Debug)]
pub enum PatternKind {
    Bool(bool),
    Some(Box<Pattern>),
    None,
    Binding(Mutable, Ident, VarId, Option<Region>),
    Deref(Box<Pattern>),
}
#[derive(Debug)]
pub struct Pattern {
    pub line: usize,
    pub kind: PatternKind,
}
#[derive(Debug)]
pub struct Expr {
    pub line: usize,
    pub kind: ExprKind,
}
#[derive(Debug)]
pub struct Param {
    pub line: usize,
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
    pub line: usize,
    pub names: Vec<Ident>,
    pub kinds: Vec<GenericKind>,
}
#[derive(Debug)]
pub struct Function {
    pub line: usize,
    pub name: Ident,
    pub generics: Option<Generics>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
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
    Param(String, usize),
    Unknown,
}
#[derive(Debug)]
pub struct Type {
    pub line: usize,
    pub kind: TypeKind,
}
#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}
