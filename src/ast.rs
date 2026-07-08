use std::fmt::Display;

use crate::{Symbol, define_id, ident::Ident, src_loc::SrcLoc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutable {
    Mutable,
    Immutable,
}
impl Mutable {
    pub const fn eq(self, other: Self) -> bool {
        match (self, other) {
            (Self::Immutable, Self::Immutable) | (Self::Mutable, Self::Mutable) => true,
            (Self::Immutable | Self::Mutable, _) => false,
        }
    }
    pub const fn usable_as(self, other: Self) -> bool {
        match (self, other) {
            (Self::Mutable, Self::Mutable)
            | (Self::Immutable, Self::Immutable)
            | (Self::Mutable, Self::Immutable) => true,
            (Self::Immutable, Self::Mutable) => false,
        }
    }
}
impl Display for Mutable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::Immutable => "imm",
            Self::Mutable => "mut",
        })
    }
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
    pub loc: SrcLoc,
    pub kind: ExprKind,
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equals,
    Lesser,
    Greater,
    And,
}
impl Display for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinaryOp::Add => f.write_str("+"),
            BinaryOp::Subtract => f.write_str("-"),
            BinaryOp::Multiply => f.write_str("*"),
            BinaryOp::Divide => f.write_str("/"),
            BinaryOp::Equals => f.write_str("=="),
            BinaryOp::Lesser => f.write_str("<"),
            BinaryOp::Greater => f.write_str(">"),
            BinaryOp::And => f.write_str("and"),
        }
    }
}
#[derive(Debug)]
pub enum Place {
    Ident(Ident),
    Deref(Box<Expr>, SrcLoc),
}
#[derive(Debug)]
pub struct Pattern {
    pub loc: SrcLoc,
    pub kind: PatternKind,
}
#[derive(Debug)]
pub struct PatternField {
    pub name: Ident,
    pub pattern: Pattern,
}
#[derive(Debug)]
pub enum PatternKind {
    Bool(bool),
    Binding(Option<Mutable>, Mutable, Ident),
    Ref(Box<Pattern>),
    Case(Ident, Option<Box<Pattern>>),
    Int(u64),
    Record(Vec<PatternField>),
    Unit,
}
#[derive(Debug)]
pub struct CaseArm {
    pub pat: Pattern,
    pub body: Expr,
}
#[derive(Debug)]
pub struct LetBinding {
    pub pattern: Pattern,
    pub ty: Option<Type>,
    pub value: Expr,
}
#[derive(Debug)]
pub struct LetExpr {
    pub binding: LetBinding,
    pub body: Expr,
}
#[derive(Debug)]
pub struct Path {
    segments: Vec<Ident>,
}
impl Path {
    pub fn new(segments: Vec<Ident>) -> Self {
        assert!(
            !segments.is_empty(),
            "Path must always have more than 1 segment"
        );
        Self { segments }
    }
    pub fn segments(&self) -> &[Ident] {
        &self.segments
    }
    pub fn head(&self) -> &Ident {
        self.segments.first().unwrap()
    }
    pub fn into_last(mut self) -> Ident {
        self.segments.pop().expect("Should have at least 1")
    }
    pub fn expect_head(self) -> Ident {
        self.into_head().expect("Expected only head")
    }
    pub fn into_head(mut self) -> Result<Ident, Self> {
        if self.segments.len() == 1 {
            Ok(self.segments.remove(0))
        } else {
            Err(self)
        }
    }
    pub fn into_segments(self) -> Vec<Ident> {
        self.segments
    }
    pub fn split_head(self) -> (Ident, Vec<Ident>) {
        let mut segments = self.into_segments();
        let head = segments.remove(0);
        (head, segments)
    }
    pub fn segments_iter(&self) -> impl IntoIterator<Item = &Ident> {
        self.segments.iter()
    }
    pub fn tail_iter(&self) -> impl IntoIterator<Item = &Ident> {
        self.segments[1..].iter()
    }
    pub fn last(&self) -> Ident {
        *self
            .segments
            .last()
            .expect("should have at least 1 segment")
    }
}
impl Display for Path {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let head = self.head();
        write!(f, "{}", head.symbol)?;
        for segment in self.tail_iter() {
            write!(f, ".{}", segment.symbol)?;
        }
        Ok(())
    }
}
impl IntoIterator for Path {
    type IntoIter = std::vec::IntoIter<Ident>;
    type Item = Ident;
    fn into_iter(self) -> Self::IntoIter {
        self.segments.into_iter()
    }
}
#[derive(Debug)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
}
#[derive(Debug)]
pub struct RecordExpr {
    pub fields: Vec<FieldInit>,
}
#[derive(Debug)]
pub struct BorrowExpr {
    pub mutable: Mutable,
    pub expr: Expr,
    pub region: Region,
}
#[derive(Debug)]
pub struct InstancePath {
    pub path: Path,
    pub generic_args: Option<GenericArgs>,
}
#[derive(Debug)]
pub enum ExprKind {
    Unit,
    Annotate(Box<Expr>, Box<Type>),
    String(String),
    Print(Option<Box<Expr>>),
    Panic,
    Call(Box<Expr>, Vec<Expr>),
    Borrow(Box<BorrowExpr>),
    Case(Box<Expr>, Vec<CaseArm>),
    While(Box<Expr>, Box<Expr>),
    For(Box<Pattern>, Box<Expr>, Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Path(InstancePath),
    Lambda(Box<Lambda>),
    Block(BlockBody, Option<Ident>),
    Deref(Box<Expr>),
    Field(Box<Expr>, Ident),
    Bool(bool),
    Number(u64),
    Record(RecordExpr),
    NamedRecord(InstancePath, Vec<FieldInit>),
    AddressOf(Box<Expr>),
    Return(Box<Expr>)
}
#[derive(Debug, Clone)]
pub struct Generics {
    pub loc: SrcLoc,
    pub names: Vec<Ident>,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy, Hash)]
pub enum IsResource {
    Resource,
    Data,
}
#[derive(Debug)]
pub struct FunctionType {
    pub resource: IsResource,
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
#[derive(Debug)]
pub struct RecordField {
    pub id: NodeId,
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct RecordType {
    pub fields: Vec<RecordField>,
}
#[derive(Debug)]
pub enum TypeKind {
    Int,
    Bool,
    String,
    Unit,
    Char,
    Record(RecordType),
    Named(InstancePath),
    Function(FunctionType),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
}
#[derive(Debug)]
pub struct Type {
    pub loc: SrcLoc,
    pub kind: TypeKind,
}
#[derive(Debug)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct Lambda {
    pub id: NodeId,
    pub params: Vec<(Ident, Option<Type>)>,
    pub resource: IsResource,
    pub body: Box<Expr>,
}
#[derive(Debug)]
pub enum AnnotationField {
    String(SrcLoc, String),
}
#[derive(Debug)]
pub struct Annotation {
    pub loc: SrcLoc,
    pub name: Ident,
    pub fields: Vec<AnnotationField>,
}
#[derive(Debug)]
pub struct GenericArg {
    pub ty: Type,
}
#[derive(Debug)]
pub struct GenericArgs {
    pub loc: SrcLoc,
    pub args: Vec<GenericArg>,
}
#[derive(Debug)]
pub struct Function {
    pub name: Ident,
    pub generics: Option<Generics>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Option<Expr>,
}
#[derive(Debug, Clone)]
pub enum Region {
    Static(SrcLoc),
    Named(Ident),
}
#[derive(Debug)]
pub struct CaseType {
    pub id: NodeId,
    pub ty: Type,
}
#[derive(Debug)]
pub struct CaseDef {
    pub id: NodeId,
    pub name: Ident,
    pub ty: Option<CaseType>,
}
#[derive(Debug)]
pub enum TypeDefKind {
    Record(RecordType),
    Variant(Vec<CaseDef>),
}

#[derive(Debug)]
pub struct Item {
    pub id: NodeId,
    pub loc: SrcLoc,
    pub annotations: Vec<Annotation>,
    pub kind: ItemKind,
}
#[derive(Debug)]
pub enum ItemKind {
    TypeDef(TypeDef),
    Function(Function),
    Import(Path, Option<Ident>),
}
#[derive(Debug)]
pub struct TypeDef {
    pub name: Ident,
    pub generics: Option<Generics>,
    pub kind: TypeDefKind,
}
define_id!(NodeId);
impl NodeId {
    pub const FIRST_ID: Self = Self(0);
}
define_id!(ModuleId);
impl ModuleId {
    pub const ROOT: Self = Self(0);
}

#[derive(Debug)]
pub struct Module {
    pub id: ModuleId,
    pub name: Symbol,
    pub items: Vec<Item>,
    pub child_modules: Vec<Module>,
}
