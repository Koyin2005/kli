use std::rc::Rc;

use crate::{
    Symbol,
    ast::{BinaryOp, IsResource, Mutable},
    def_ids::DefId,
    define_id,
    ident::Ident,
    index_vec::IndexVec,
    lang_items::LangItem,
    src_loc::SrcLoc,
    typed_ast::FieldId,
};
#[derive(Debug, PartialEq, Eq)]
pub struct FunctionDefId(pub DefId);
define_id!(VarId);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Var(pub Symbol, pub VarId);
impl Var {
    pub fn ident(self, loc: SrcLoc) -> Ident {
        Ident {
            symbol: self.0,
            loc,
        }
    }
}
#[derive(Debug)]
pub struct Signature {
    pub params: Box<[Type]>,
    pub return_type: Type,
}
#[derive(Debug)]
pub struct Lambda {
    pub id: DefId,
    pub loc: SrcLoc,
    pub resource: IsResource,
    pub param_tys: Box<[Option<Type>]>,
    pub params: Box<[Param]>,
    pub body: Expr,
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
    pub stmts: Box<[Stmt]>,
    pub expr: Box<Expr>,
}
#[derive(Debug)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
}
#[derive(Debug)]
pub enum GenericArg {
    Type(Type),
}
impl GenericArg {
    pub fn loc(&self) -> SrcLoc {
        match self {
            Self::Type(ty) => ty.loc,
        }
    }
}
#[derive(Debug)]
pub struct GenericArgs {
    pub loc: Option<SrcLoc>,
    pub args: Vec<GenericArg>,
}
impl GenericArgs {
    pub const NONE: Self = Self {
        loc: None,
        args: Vec::new(),
    };
    pub fn len(&self) -> usize {
        self.args.len()
    }
    pub fn args(&self) -> Option<&[GenericArg]> {
        if self.loc.is_some() {
            Some(&self.args)
        } else {
            None
        }
    }
}
#[derive(Debug)]
pub struct ForExpr {
    pub pattern: Pattern,
    pub iterator: Expr,
    pub body: Expr,
}
#[derive(Debug)]
pub enum ExprKind {
    Unsafe(Box<Expr>),
    Block(Box<BlockBody>),
    Unit,
    Err,
    Annotate(Box<Expr>, Box<Type>),
    Int(IntegerLiteral),
    Bool(bool),
    String(Rc<str>),
    Var(Var),
    Function(FunctionDefId, Box<GenericArgs>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Panic,
    Lambda(Rc<Lambda>),
    Deref(Box<Expr>),
    Assign(Box<Expr>, Box<Expr>),
    For(Box<ForExpr>),
    Case(Box<Expr>, Box<[CaseArm]>),
    Print(Option<Box<Expr>>),
    Call(Box<Expr>, Box<[Expr]>),
    Record(Vec<FieldInit>),
    VariantCase(DefId, Box<GenericArgs>),
    AddressOf(Box<Expr>),
    Field(Box<Expr>, Ident),
    NamedRecord(TypeName, Box<GenericArgs>, Box<[FieldInit]>),
    While(Box<Expr>, Box<Expr>),
    Tuple(Vec<Expr>),
    Return(Box<Expr>),
    MethodCall(Box<Expr>, Ident, Box<[Expr]>),
    TypeRelativePath(TypeName, Ident, Box<GenericArgs>),
}

#[derive(Debug)]
pub struct PatternField {
    pub name: Ident,
    pub pattern: Pattern,
}
#[derive(Debug, Clone, Copy)]
pub struct IntegerLiteral {
    pub value: u64,
    pub kind: IntegerLiteralKind,
}
#[derive(Debug, Clone, Copy)]
pub enum IntegerLiteralKind {
    Signed,
    Unsigned,
    Implicit,
}
#[derive(Debug)]
pub enum PatternKind {
    Int(IntegerLiteral),
    Bool(bool),
    Case(Ident, Option<Box<Pattern>>),
    Binding(Option<Mutable>, Mutable, Ident, VarId),
    Record(Box<[PatternField]>),
    Tuple(Box<[Pattern]>),
    Unit,
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
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenericKind {
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
    pub name: Ident,
    pub generics: Option<Box<Generics>>,
    pub param_tys: Box<[Type]>,
    pub return_type: Box<Type>,
    pub params: Box<[Param]>,
    pub body: Option<Box<Expr>>,
}
#[derive(Debug)]
pub struct RecordFieldType {
    pub name: Ident,
    pub ty: Type,
}

#[derive(Debug)]
pub struct FunctionType {
    pub is_resource: IsResource,
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeName {
    Int,
    Uint,
    Bool,
    String,
    Char,
    Ptr,
    Byte,
    UserDefined(DefId),
    Box,
    ArrayList,
    Param(Symbol, usize),
    Never,
    Pair,
}
#[derive(Debug)]
pub enum TypeKind {
    Function(Box<FunctionType>),
    Named(TypeName, Box<GenericArgs>),
    Unknown,
    Record(Box<[RecordFieldType]>),
    Tuple(Vec<Type>),
}
#[derive(Debug)]
pub struct Type {
    pub loc: SrcLoc,
    pub kind: TypeKind,
}
#[derive(Debug)]
pub struct CaseField {
    pub id: DefId,
    pub ty: Type,
}
#[derive(Debug, Clone, Copy)]
pub struct CaseDef {
    pub id: DefId,
    pub name: Ident,
    pub field: Option<DefId>,
}
#[derive(Debug)]
pub struct FieldDef {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct RecordDef {
    pub fields: IndexVec<FieldId, DefId>,
}
#[derive(Debug)]
pub struct VariantDef {
    pub cases: Vec<CaseDef>,
}
#[derive(Debug)]
pub enum TypeDefKind {
    Variant(VariantDef),
    Record(RecordDef),
}
#[derive(Debug)]
pub struct TypeImpl {
    pub ty: DefId,
    pub methods: Vec<DefId>,
}
#[derive(Debug)]
pub struct TypeDef {
    pub name: Ident,
    pub generics: Option<Box<Generics>>,
    pub impl_: Option<DefId>,
    pub kind: TypeDefKind,
}
impl TypeDef {
    #[track_caller]
    pub fn expect_variant(&self) -> &VariantDef {
        let TypeDefKind::Variant(ref variant) = self.kind else {
            unreachable!("expected a variant def")
        };
        variant
    }
    #[track_caller]
    pub fn expect_record(&self) -> &RecordDef {
        let TypeDefKind::Record(ref record) = self.kind else {
            unreachable!("expected a record def")
        };
        record
    }
}
#[derive(Debug)]
pub struct Module {
    pub name: Ident,
    pub items: Box<[DefId]>,
}
#[derive(Debug)]
pub struct ImportTree {
    pub name: Ident,
    pub children: Box<[ImportTree]>,
}
#[derive(Debug)]
pub enum ItemKind {
    TypeDef(Box<TypeDef>),
    Function(Box<Function>),
    Module(Box<Module>),
    Import(Box<ImportTree>),
}
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum AnnotationKind {
    Copy,
    Unsafe,
    LangItem(LangItem),
    Opaque,
}
#[derive(Debug)]
pub struct Annotation {
    pub loc: SrcLoc,
    pub kind: AnnotationKind,
}
impl Annotation {
    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            AnnotationKind::Copy => "copy",
            AnnotationKind::LangItem(_) => "lang_item",
            AnnotationKind::Opaque => "opaque",
            AnnotationKind::Unsafe => "unsafe",
        }
    }
}
#[derive(Debug)]
pub struct Item {
    pub id: DefId,
    pub annotations: Box<[Annotation]>,
    pub loc: SrcLoc,
    pub kind: ItemKind,
}
impl Item {
    pub fn kind_str(&self) -> &'static str {
        match &self.kind {
            ItemKind::TypeDef(_) => "type def",
            ItemKind::Function(_) => "function def",
            ItemKind::Module(_) => "module def",
            ItemKind::Import(_) => "import",
        }
    }
    pub fn ident(&self) -> Ident {
        match &self.kind {
            ItemKind::Function(function) => function.name,
            ItemKind::Module(module) => module.name,
            ItemKind::TypeDef(type_def) => type_def.name,
            ItemKind::Import(import) => import.name,
        }
    }
    #[track_caller]
    pub fn expect_type_def(&self) -> &TypeDef {
        match self.kind {
            ItemKind::TypeDef(ref type_def) => type_def,
            _ => panic!("expected a type def but got {:?}", self),
        }
    }
    pub fn function_def(&self) -> Option<&Function> {
        match &self.kind {
            ItemKind::Function(function) => Some(function),
            _ => None,
        }
    }
    #[track_caller]
    pub fn expect_function_def(&self) -> &Function {
        self.function_def().expect("should be a function")
    }
}

#[derive(Debug)]
pub struct Method {
    pub annotations: Vec<Annotation>,
    pub function: Function,
}
#[derive(Debug)]
pub enum Node {
    Method(Box<Method>),
    Item(Box<Item>),
    Lambda(Rc<Lambda>),
    Field(Box<FieldDef>),
    Case(Box<CaseDef>),
    CaseField(Box<CaseField>),
    Impl(Box<TypeImpl>),
}

impl Node {
    pub fn kind(&self) -> &'static str {
        match self {
            Node::Method(_) => "method",
            Node::Item(item) => item.kind_str(),
            Node::Lambda(_) => "lambda",
            Node::Field(_) => "field",
            Node::Case(_) => "case",
            Node::CaseField(_) => "case field",
            Self::Impl(_) => "impl",
        }
    }
    pub fn item(&self) -> Option<&Item> {
        match self {
            Self::Item(item) => Some(item),
            _ => None,
        }
    }
    #[track_caller]
    pub fn expect_item(&self) -> &Item {
        self.item().expect("should be a valid item")
    }
    pub fn lambda(&self) -> Option<&Lambda> {
        match self {
            Self::Lambda(lambda) => Some(lambda),
            _ => None,
        }
    }
    #[track_caller]
    pub fn expect_lambda(&self) -> &Lambda {
        self.lambda().expect("should be a valid lambda")
    }
    #[track_caller]
    pub fn expect_field(&self) -> &FieldDef {
        match self {
            Self::Field(field) => field,
            _ => panic!("expected a valid field"),
        }
    }
    #[track_caller]
    pub fn expect_case_field(&self) -> &CaseField {
        match self {
            Self::CaseField(field) => field,
            _ => panic!("expected a valid field"),
        }
    }
}
