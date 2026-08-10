use std::{collections::BTreeMap, rc::Rc};

use crate::{
    ast::Mutable,
    builtins::Builtin,
    def_ids::DefId,
    define_id,
    ident::Ident,
    resolved_ast::{Var, VarId},
    src_loc::SrcLoc,
    types::{CaseId, GenericArgs, GenericKind, Type},
};

#[derive(Debug)]
pub struct PatternField<'ctxt> {
    pub index: FieldId,
    pub pattern: Pattern<'ctxt>,
}
#[derive(Debug)]
pub struct Pattern<'ctxt> {
    pub ty: Type<'ctxt>,
    pub loc: SrcLoc,
    pub kind: PatternKind<'ctxt>,
}
#[derive(Debug)]
pub enum PatternKind<'ctxt> {
    Err,
    Unit,
    Int(u64),
    Bool(bool),
    Case(
        DefId,
        GenericArgs<'ctxt>,
        CaseId,
        Option<Box<Pattern<'ctxt>>>,
    ),
    Binding(Mutable, Var, Type<'ctxt>),
    Record(Vec<PatternField<'ctxt>>),
}
#[derive(Debug)]
pub struct Place<'ctxt> {
    pub ty: Type<'ctxt>,
    pub loc: SrcLoc,
    pub kind: PlaceKind<'ctxt>,
}
#[derive(Debug)]
pub enum PlaceKind<'ctxt> {
    Upvar(DefId, Var),
    Var(Var),
    Field(Box<Place<'ctxt>>, FieldId),
    Index(Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
    Deref(Box<Expr<'ctxt>>),
    Invalid,
}
#[derive(Debug, Clone)]
pub struct Capture<'ctxt> {
    pub var: Var,
    pub ty: Type<'ctxt>,
}
#[derive(Debug)]
pub struct LambdaParam {
    pub var: Var,
    pub loc: SrcLoc,
}
#[derive(Debug)]
pub struct Lambda<'ctxt> {
    pub id: DefId,
    pub loc: SrcLoc,
    pub params: Vec<LambdaParam>,
    pub param_tys: Vec<Type<'ctxt>>,
    pub return_type: Type<'ctxt>,
}
#[derive(Debug)]
pub struct LetBinding<'ctxt> {
    pub pattern: Pattern<'ctxt>,
    pub value: Expr<'ctxt>,
}
#[derive(Debug)]
pub enum StmtKind<'ctxt> {
    Let(LetBinding<'ctxt>),
    Expr(Expr<'ctxt>),
}
#[derive(Debug)]
pub struct Stmt<'ctxt> {
    pub loc: SrcLoc,
    pub kind: StmtKind<'ctxt>,
}
#[derive(Debug)]
pub struct BlockBody<'ctxt> {
    pub stmts: Vec<Stmt<'ctxt>>,
    pub expr: Box<Expr<'ctxt>>,
}
#[derive(Debug)]
pub struct Expr<'ctxt> {
    pub ty: Type<'ctxt>,
    pub loc: SrcLoc,
    pub kind: ExprKind<'ctxt>,
}
define_id!(FieldId);
impl FieldId {
    pub const FIRST_FIELD: Self = Self(0);
}

#[derive(Debug)]
pub struct RecordFieldInit<'ctxt> {
    pub index: FieldId,
    pub value: Expr<'ctxt>,
}
#[derive(Debug)]
pub enum IteratorType {}
#[derive(Debug)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equals,
    Greater,
    Lesser,
    BitwiseOr,
    BitwiseAnd,
}

#[derive(Debug)]
pub enum LogicalOp {
    And,
    Or,
}
#[derive(Debug)]
pub enum ExprKind<'ctxt> {
    Unsafe(Box<Expr<'ctxt>>),
    Return(Box<Expr<'ctxt>>),
    Block(BlockBody<'ctxt>),
    String(Rc<str>),
    Bool(bool),
    Int(u64),
    Char(char),
    Unit,
    Err,
    Panic,
    NeverToAny(Box<Expr<'ctxt>>),
    BuiltinCall(DefId, Builtin, GenericArgs<'ctxt>, Box<[Expr<'ctxt>]>),
    VariantInit(DefId, CaseId, GenericArgs<'ctxt>, Option<Box<Expr<'ctxt>>>),
    Function(DefId, GenericArgs<'ctxt>),
    Const(DefId, GenericArgs<'ctxt>),
    Call(Box<Expr<'ctxt>>, Vec<Expr<'ctxt>>),
    Load(Place<'ctxt>),
    Binary(BinaryOp, Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
    Logic(LogicalOp, Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
    For {
        pattern: Box<Pattern<'ctxt>>,
        iterator: Box<Expr<'ctxt>>,
        iterator_type: IteratorType,
        body: Box<Expr<'ctxt>>,
    },
    Case(Box<Expr<'ctxt>>, Vec<CaseArm<'ctxt>>),
    Assign(Box<Place<'ctxt>>, Box<Expr<'ctxt>>),
    Lambda(Box<Lambda<'ctxt>>),
    Tuple(Box<[Expr<'ctxt>]>),
    Array(Box<[Expr<'ctxt>]>),
    NamedRecord(DefId, GenericArgs<'ctxt>, Box<[RecordFieldInit<'ctxt>]>),
    While(Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
}
#[derive(Debug)]
pub struct CaseArm<'ctxt> {
    pub pattern: Pattern<'ctxt>,
    pub body: Expr<'ctxt>,
}
pub struct GenericParam {
    pub name: Ident,
    pub kind: GenericKind,
}
#[derive(Debug, Clone)]
pub struct Param<'ctxt> {
    pub name: Ident,
    pub var: Option<VarId>,
    pub ty: Type<'ctxt>,
}
impl<'ctxt> Param<'ctxt> {
    pub fn var(&self) -> Option<Var> {
        Some(Var(self.name.symbol, self.var?))
    }
}
pub struct Function<'ctxt> {
    pub params: Vec<Param<'ctxt>>,
    pub return_type: Type<'ctxt>,
    pub body: Option<Expr<'ctxt>>,
}

pub struct Program<'ctxt> {
    pub functions: BTreeMap<DefId, Function<'ctxt>>,
}
