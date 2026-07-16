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
define_id!(LocalRegionId);
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
pub struct BorrowExpr {
    pub mutable: Mutable,
    pub place: Expr,
    pub region: Region,
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
    Region(Region),
}
impl GenericArg {
    pub fn loc(&self) -> SrcLoc {
        match self {
            Self::Type(ty) => ty.loc,
            Self::Region(region) => region.loc,
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
    Block(Box<BlockBody>, Option<LocalRegionId>),
    Unit,
    Err,
    Annotate(Box<Expr>, Box<Type>),
    Int(i64),
    Bool(bool),
    String(Rc<str>),
    Var(Var),
    Function(FunctionDefId, Box<GenericArgs>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Borrow(Box<BorrowExpr>),
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
}
#[derive(Debug, Clone, Copy)]
pub enum RegionKind {
    Param(Symbol, usize),
    Local(Symbol, LocalRegionId),
    Static,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub loc: SrcLoc,
    pub kind: RegionKind,
}

#[derive(Debug)]
pub struct PatternField {
    pub name: Ident,
    pub pattern: Pattern,
}
#[derive(Debug)]
pub enum PatternKind {
    Int(i64),
    Bool(bool),
    Ref(Box<Pattern>),
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
    Region,
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
    Unit,
    Int,
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
    Ptr(Box<Type>),
    Imm(Box<Region>, Box<Type>),
    Mut(Box<Region>, Box<Type>),
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
pub struct TypeDef {
    pub name: Ident,
    pub generics: Option<Box<Generics>>,
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
pub enum ItemKind {
    TypeDef(Box<TypeDef>),
    Function(Box<Function>),
    Module(Box<Module>),
    Import(Ident),
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
            ItemKind::Import(name) => *name,
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
pub enum Node {
    Item(Box<Item>),
    Lambda(Rc<Lambda>),
    Field(Box<FieldDef>),
    Case(Box<CaseDef>),
    CaseField(Box<CaseField>),
}

impl Node {
    pub fn kind(&self) -> &'static str {
        match self {
            Node::Item(item) => item.kind_str(),
            Node::Lambda(_) => "lambda",
            Node::Field(_) => "field",
            Node::Case(_) => "case",
            Node::CaseField(_) => "case field",
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
