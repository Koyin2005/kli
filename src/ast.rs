#[derive(Debug,Clone)]
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
    Binding(Mutable, Ident, Option<Region>),
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
    Lambda(Lambda),
    Deref(Box<Expr>),
    Number(u64),
}
#[derive(Debug,Clone)]
pub struct Generics {
    pub line: usize,
    pub names: Vec<Ident>,
}
#[derive(Debug,Clone)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
#[derive(Debug,Clone)]
pub enum Type {
    Int,
    Bool,
    String,
    Unit,
    Named(Ident),
    Closure(FunctionType),
    Function(FunctionType),
    Option(Box<Type>),
    List(Box<Type>),
    Ref(Box<Type>),
    Imm(Option<Region>, Box<Type>),
    Mut(Option<Region>, Box<Type>),
}
#[derive(Debug,Clone)]
pub struct Param {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct Lambda {
    pub params: Vec<(Ident, Option<Type>)>,
    pub return_type: Option<Type>,
    pub body: Box<Expr>,
}
#[derive(Debug)]
pub struct Function {
    pub name: Ident,
    pub generics: Option<Generics>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Expr,
}
#[derive(Debug,Clone)]
pub enum Region {
    Static(usize),
    Named(Ident),
}

#[derive(Debug)]
pub struct Program {
    pub functions: Vec<Function>,
}
