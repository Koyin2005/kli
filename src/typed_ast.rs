use std::{collections::BTreeMap, rc::Rc};

use crate::{
    ast::{IsResource, Mutable},
    builtins::Builtin,
    def_ids::DefId,
    define_id,
    ident::Ident,
    resolved_ast::{LocalRegionId, Var, VarId},
    src_loc::SrcLoc,
    types::{CaseId, GenericArgs, GenericKind, Region, Type},
};

#[derive(Debug)]
pub struct PatternField {
    pub index: FieldId,
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
    Err,
    Unit,
    Int(i64),
    Bool(bool),
    Ref(Box<Pattern>),
    Case(DefId, GenericArgs, CaseId, Option<Box<Pattern>>),
    Binding(Option<(Mutable, Region)>, Mutable, Var, Box<Type>),
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
    Upvar(DefId, Var),
    Var(Var),
    Deref(Box<Expr>),
    Field(Box<Place>, FieldId),
    Invalid,
}
#[derive(Debug, Clone)]
pub struct Capture {
    pub var: Var,
    pub ty: Type,
}
#[derive(Debug)]
pub struct LambdaParam {
    pub var: Var,
    pub loc: SrcLoc,
}
#[derive(Debug)]
pub struct Lambda {
    pub id: DefId,
    pub loc: SrcLoc,
    pub is_resource: IsResource,
    pub captures: Vec<Capture>,
    pub params: Vec<LambdaParam>,
    pub param_tys: Vec<Type>,
    pub return_type: Box<Type>,
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
impl FieldId {
    pub const FIRST_FIELD: Self = Self(0);
}

#[derive(Debug)]
pub struct RecordFieldInit {
    pub index: FieldId,
    pub value: Expr,
}
#[derive(Debug)]
pub enum IteratorType {
    ArrayListRef(Region, Mutable, Type),
    StringIter(Region, Mutable),
}
#[derive(Debug)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equals,
    Greater,
    Lesser,
}

#[derive(Debug)]
pub enum LogicalOp {
    And,
}
#[derive(Debug)]
pub enum ExprKind {
    Return(Box<Expr>),
    Record(Box<[RecordFieldInit]>),
    Block(BlockBody, Option<LocalRegionId>),
    String(Rc<str>),
    Bool(bool),
    Int(i64),
    Unit,
    Err,
    Panic,
    NeverToAny(Box<Expr>),
    BuiltinCall(Builtin, GenericArgs, Box<[Expr]>),
    VariantInit(DefId, CaseId, GenericArgs, Option<Box<Expr>>),
    Function(DefId, GenericArgs),
    Const(DefId, GenericArgs),
    Print(Option<Box<Expr>>),
    Call(Box<Expr>, Vec<Expr>),
    Load(Place),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Logic(LogicalOp, Box<Expr>, Box<Expr>),
    For {
        pattern: Box<Pattern>,
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
    Assign(Box<Place>, Box<Expr>),
    Lambda(Box<Lambda>),
    AddressOf(Box<Place>),
    NamedRecord(DefId, GenericArgs, Box<[RecordFieldInit]>),
    While(Box<Expr>, Box<Expr>),
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
    pub var: Option<VarId>,
    pub ty: Type,
}
impl Param {
    pub fn var(&self) -> Option<Var> {
        Some(Var(self.name.symbol, self.var?))
    }
}
pub struct Function {
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Option<Expr>,
}

pub struct Program {
    pub functions: BTreeMap<DefId, Function>,
}
