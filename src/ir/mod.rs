use crate::{Symbol, define_id, index_vec::IndexVec, typed_ast::FieldId, types::CaseId};
pub mod lower;
mod print;
#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Bool(bool),
    Function(BodyId, Vec<Type>),
    String(Symbol),
    Char(char),
}
#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
}
impl Expr {
    pub fn len(self) -> Self {
        Self {
            kind: ExprKind::Len(Box::new(self)),
        }
    }
    pub fn not(self) -> Self {
        Self {
            kind: ExprKind::Not(Box::new(self)),
        }
    }
    pub fn tuple(exprs: impl IntoIterator<Item = Self>) -> Self {
        Self::aggregate(AggregateKind::Tuple, exprs)
    }
    pub fn aggregate(kind: AggregateKind, exprs: impl IntoIterator<Item = Self>) -> Self {
        Self {
            kind: ExprKind::Aggregate(kind, exprs.into_iter().collect()),
        }
    }
    pub fn unit_value() -> Self {
        Self {
            kind: ExprKind::Aggregate(AggregateKind::Tuple, IndexVec::new()),
        }
    }
    pub fn constant(value: Constant) -> Self {
        Self {
            kind: ExprKind::Constant(value),
        }
    }
    pub fn load(place: Place) -> Self {
        Self {
            kind: ExprKind::Load(place),
        }
    }
    pub fn load_local(local: Local) -> Self {
        Self {
            kind: ExprKind::Load(Place::Local(local)),
        }
    }
    pub fn binary(op: BinaryOp, left: Self, right: Self) -> Self {
        Self {
            kind: ExprKind::BinaryOp(op, Box::new(left), Box::new(right)),
        }
    }
}
#[derive(Debug, Clone)]
pub enum AggregateKind {
    Tuple,
    Named,
    Variant(TypeDefId, CaseId, Vec<Type>),
}
#[derive(Debug, Clone, Copy)]
pub enum BinaryOp {
    Add,
    AddWithOverflow,
    Lesser,
    Greater,
    Equals,
    InBounds,
}
#[derive(Debug, Clone)]
pub enum ExprKind {
    Constant(Constant),
    Load(Place),
    Len(Box<Expr>),
    Discriminant(Place),
    Aggregate(AggregateKind, IndexVec<FieldId, Expr>),
    BinaryOp(BinaryOp, Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
}
define_id!(Local);
#[derive(Debug, Clone)]
pub enum Place {
    Local(Local),
    Field(Box<Place>, FieldId),
    Deref(Box<Place>),
    Downcast(Box<Place>, CaseId),
    Index(Box<Place>, Box<Expr>),
}
impl Place {
    pub fn with_field(self, field: FieldId) -> Self {
        Self::Field(Box::new(self), field)
    }
}

#[derive(Debug)]
pub struct SwitchArm {
    pub value: i32,
    pub body: DecisionTree,
}
#[derive(Debug)]
pub struct Switch {
    pub value: Expr,
    pub arms: Vec<SwitchArm>,
    pub otherwise: Box<DecisionTree>,
}
#[derive(Debug)]
pub struct Call {
    pub return_place: Place,
    pub callee: Expr,
    pub args: Vec<Expr>,
}
define_id!(ArmId);
#[derive(Debug)]
pub enum DecisionTree {
    Body(ArmId),
    Switch(Switch),
}
define_id!(LoopLabel);
#[derive(Debug)]
pub struct Loop {
    pub label: LoopLabel,
    pub stmts: Vec<Stmt>,
}
#[derive(Debug)]
pub struct Match {
    pub tree: DecisionTree,
    pub arms: IndexVec<ArmId, Vec<Stmt>>,
}
#[derive(Debug)]
pub enum Allocate {
    Array(Type, Vec<Expr>),
}
#[derive(Debug)]
pub enum Stmt {
    Panic,
    PanicIf(Expr),
    Print { value: Expr, is_err: bool },
    Loop(LoopLabel, Vec<Stmt>),
    Break(LoopLabel),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Match(Match),
    Call(Call),
    ReadLine(Place),
    Alloc(Place, Allocate),
    Assign(Place, Expr),
    Return(Expr),
}

define_id!(TypeDefId);
#[derive(Debug, Clone)]
pub enum Type {
    Int,
    Bool,
    String,
    Char,
    Never,
    Param(u32),
    Function(Vec<Type>, Box<Type>),
    Tuple(Vec<Type>),
    Array(Box<Type>),
    Named(TypeDefId, Vec<Type>),
    Box(Box<Type>),
}
impl Type {
    pub fn format_type(&self, _: &Program) -> String {
        format!("{self:?}")
    }
    pub fn format_generic_args(args: &[Self], program: &Program) -> String {
        if args.is_empty() {
            return String::new();
        }
        let mut output = "[".to_string();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                output.push(',');
            }
            output.push_str(&arg.format_type(program));
        }
        output.push(']');
        output
    }
}
#[derive(Debug)]
pub struct LocalInfo {
    pub name: Option<Symbol>,
    pub is_mutable: bool,
    pub ty: Type,
}
#[derive(Debug)]
pub struct Body {
    pub name: String,
    pub generic_params: Vec<Symbol>,
    pub param_count: u32,
    pub return_ty: Type,
    pub locals: IndexVec<Local, LocalInfo>,
    pub body: Vec<Stmt>,
}
pub struct CaseField {
    pub ty: Type,
}
pub struct CaseDef {
    pub name: String,
    pub field: Option<CaseField>,
}
pub struct VariantDef {
    pub cases: IndexVec<CaseId, CaseDef>,
}
define_id!(BodyId);
pub enum TypeDef {
    Variant(VariantDef),
    Struct,
}
impl TypeDef {
    #[track_caller]
    pub fn variant_def(&self) -> &VariantDef {
        let Self::Variant(variant_def) = self else {
            panic!("Should be a variant def")
        };
        variant_def
    }
}
#[derive(Default)]
pub struct Program {
    pub type_defs: IndexVec<TypeDefId, TypeDef>,
    pub entrypoint: Option<BodyId>,
    pub bodies: IndexVec<BodyId, Body>,
}
