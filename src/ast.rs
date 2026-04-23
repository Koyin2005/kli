#[derive(Debug)]
pub struct Ident {
    pub content: String,
    pub line: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutable {
    Mutable,
    Immutable,
}
#[derive(Debug)]
pub struct Expr {
    pub line: usize,
    pub kind: ExprKind,
}
impl Expr {
    pub fn as_place(self) -> Result<Place, Expr> {
        match self.kind {
            ExprKind::Ident(name) => Ok(Place::Ident(name)),
            ExprKind::Deref(expr) => {
                let place = expr.as_place()?;
                Ok(Place::Deref(Box::new(place)))
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
    Deref(Box<Place>),
}
#[derive(Debug)]
pub struct Pattern {
    pub line: usize,
    pub kind: PatternKind,
}
#[derive(Debug)]
pub enum PatternKind {
    Binding(Mutable, Ident, Option<Ident>),
    Some(Box<Pattern>),
    None,
}
#[derive(Debug)]
pub struct CaseArm {
    pub pat: Pattern,
    pub body: Expr,
}
#[derive(Debug)]
pub enum ExprKind {
    Unit,
    List(Vec<Expr>),
    String(String),
    Print(Option<Box<Expr>>),
    Panic(Option<Type>),
    Some(Box<Expr>),
    None(Option<Type>),
    Call(Box<Expr>, Vec<Expr>),
    Borrow(Mutable, Ident, Ident, Box<Expr>),
    Case(Box<Expr>, Vec<CaseArm>),
    Let(Mutable, Ident, Box<Expr>, Option<Type>, Box<Expr>),
    Sequence(Box<Expr>, Box<Expr>),
    For(Mutable, Ident, Box<Expr>, Box<Expr>),
    Assign(Place, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Ident(Ident),
    Deref(Box<Expr>),
    Number(u64),
}
#[derive(Debug)]
pub struct Generics {
    pub line: usize,
    pub names: Vec<Ident>,
}
#[derive(Debug)]
pub enum Type {
    Int,
    Bool,
    String,
    Unit,
    Option(Box<Type>),
    List(Box<Type>),
    Imm(Option<Ident>, Box<Type>),
    Mut(Option<Ident>, Box<Type>),
}
#[derive(Debug)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct Function {
    pub name: Ident,
    pub generics: Option<Generics>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}
