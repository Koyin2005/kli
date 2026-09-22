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
impl<'ctxt> Pattern<'ctxt> {
    pub fn binding(ty: Type<'ctxt>, loc: SrcLoc, mutable: Mutable, var: Var) -> Self {
        Self {
            ty,
            loc,
            kind: PatternKind::Binding(mutable, var, ty),
        }
    }
}
#[derive(Debug)]
pub enum PatternKind<'ctxt> {
    Err,
    Unit,
    Int(u64),
    Bool(bool),
    Char(char),
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
impl<'ctxt> Place<'ctxt> {
    pub fn var(ty: Type<'ctxt>, loc: SrcLoc, var: Var) -> Self {
        Self {
            ty,
            loc,
            kind: PlaceKind::Var(var),
        }
    }
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
impl<'ctxt> Stmt<'ctxt> {
    pub fn expr(loc: SrcLoc, expr: Expr<'ctxt>) -> Self {
        Self {
            loc,
            kind: StmtKind::Expr(expr),
        }
    }
    pub fn let_stmt(loc: SrcLoc, pattern: Pattern<'ctxt>, value: Expr<'ctxt>) -> Self {
        Self {
            loc,
            kind: StmtKind::Let(LetBinding { pattern, value }),
        }
    }
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
impl<'ctxt> Expr<'ctxt> {
    pub fn assign(ty: Type<'ctxt>, loc: SrcLoc, left: Place<'ctxt>, right: Self) -> Self {
        Self {
            ty,
            loc,
            kind: ExprKind::Assign(Box::new(left), Box::new(right)),
        }
    }
    pub fn binary(ty: Type<'ctxt>, loc: SrcLoc, op: BinaryOp, left: Self, right: Self) -> Self {
        Self {
            ty,
            loc,
            kind: ExprKind::Binary(op, Box::new(left), Box::new(right)),
        }
    }
    pub fn var(ty: Type<'ctxt>, loc: SrcLoc, var: Var) -> Self {
        Self {
            ty,
            loc,
            kind: ExprKind::Load(Place::var(ty, loc, var)),
        }
    }
    pub fn block(ty: Type<'ctxt>, loc: SrcLoc, stmts: Vec<Stmt<'ctxt>>, result: Self) -> Self {
        Self {
            ty,
            loc,
            kind: ExprKind::Block(BlockBody {
                stmts,
                expr: Box::new(result),
            }),
        }
    }
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

#[derive(Debug, Clone, Copy)]
pub enum LogicalOp {
    And,
    Or,
}
#[derive(Debug)]
pub enum ExprKind<'ctxt> {
    If(Box<Expr<'ctxt>>, Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
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
    BuiltinCall(Builtin, GenericArgs<'ctxt>, Box<[Expr<'ctxt>]>),
    VariantInit(DefId, CaseId, GenericArgs<'ctxt>, Option<Box<Expr<'ctxt>>>),
    Function(DefId, GenericArgs<'ctxt>),
    VariantConstructor {
        ty: DefId,
        args: GenericArgs<'ctxt>,
        case: CaseId,
    },
    Call(Box<Expr<'ctxt>>, Vec<Expr<'ctxt>>),
    Load(Place<'ctxt>),
    Binary(BinaryOp, Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
    Logic(LogicalOp, Box<Expr<'ctxt>>, Box<Expr<'ctxt>>),
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
