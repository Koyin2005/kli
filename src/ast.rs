use std::fmt::Display;

use crate::{ident::Ident, src_loc::SrcLoc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutable {
    Mutable,
    Immutable,
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
impl Expr {
    pub fn as_place(self) -> Result<Place, Expr> {
        match self.kind {
            ExprKind::Path(path) => match path.into_head() {
                Ok(head) => Ok(Place::Ident(head)),
                Err(path) => Err(Expr {
                    loc: self.loc,
                    kind: ExprKind::Path(path),
                }),
            },
            ExprKind::Deref(expr) => {
                let loc = expr.loc.clone();
                Ok(Place::Deref(Box::new(*expr), loc))
            }
            _ => Err(self),
        }
    }
}
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
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
pub enum PatternKind {
    Bool(bool),
    Binding(Mutable, Ident, Option<Region>),
    Some(Box<Pattern>),
    None,
    Deref(Box<Pattern>),
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
    pub fn display(&self) -> impl Display {
        struct DisplayPath<'a> {
            path: &'a Path,
        }
        impl Display for DisplayPath<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let head = self.path.head();
                f.write_str(&head.content)?;
                for segment in self.path.tail_iter() {
                    f.write_str(".")?;
                    f.write_str(&segment.content)?;
                }
                Ok(())
            }
        }
        DisplayPath { path: self }
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
    pub var_name: Ident,
    pub region: Ident,
    pub body: Box<Expr>,
}
#[derive(Debug)]
pub enum ExprKind {
    Unit,
    Annotate(Box<Expr>, Box<Type>),
    List(Vec<Expr>),
    String(String),
    Print(Option<Box<Expr>>),
    Panic(Option<Type>),
    Some(Box<Expr>),
    None(Option<Type>),
    Call(Box<Expr>, Vec<Expr>),
    Borrow(Box<BorrowExpr>),
    Case(Box<Expr>, Vec<CaseArm>),
    For(Box<Pattern>, Box<Expr>, Box<Expr>),
    Assign(Place, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Path(Path),
    Lambda(Box<Lambda>),
    Block(BlockBody),
    Deref(Box<Expr>),
    Bool(bool),
    Number(u64),
    Record(RecordExpr),
}
#[derive(Debug, Clone)]
pub struct Generics {
    pub loc: SrcLoc,
    pub names: Vec<Ident>,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum IsResource {
    Resource,
    Data,
}
#[derive(Debug, Clone)]
pub struct FunctionType {
    pub resource: IsResource,
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
#[derive(Debug, Clone)]
pub struct RecordField {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug, Clone)]
pub struct RecordType {
    pub fields: Vec<RecordField>,
}
#[derive(Debug, Clone)]
pub enum TypeKind {
    Int,
    Bool,
    String,
    Unit,
    Char,
    Record(RecordType),
    Named(Ident),
    Function(FunctionType),
    Option(Box<Type>),
    List(Box<Type>),
    Box(Box<Type>),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
}
#[derive(Debug, Clone)]
pub struct Type {
    pub loc: SrcLoc,
    pub kind: TypeKind,
}
#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct Lambda {
    pub params: Vec<(Ident, Option<Type>)>,
    pub resource: IsResource,
    pub body: Box<Expr>,
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
#[derive(Debug, Clone)]
pub enum Region {
    Static(SrcLoc),
    Named(Ident),
}

#[derive(Debug)]
pub struct Module {
    pub functions: Vec<Function>,
}
