use std::{collections::HashMap, rc::Rc};

use crate::{
    Symbol,
    ast::{BinaryOp, IsResource, Mutable},
    define_id,
    ident::Ident,
    index_vec::IndexVec,
    src_loc::SrcLoc,
    typed_ast::FieldId,
};
#[derive(Debug, PartialEq, Eq)]
pub struct FunctionDefId(pub DefId);
define_id!(LambdaId);
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub struct VarId(usize);
impl VarId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }
}
impl From<VarId> for usize {
    fn from(value: VarId) -> Self {
        value.0
    }
}
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub struct LocalRegionId(usize);
impl LocalRegionId {
    pub fn new(index: usize) -> Self {
        Self(index)
    }
}
impl From<LocalRegionId> for usize {
    fn from(value: LocalRegionId) -> Self {
        value.0
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Var(pub Symbol, pub VarId);
#[derive(Debug)]
pub struct BorrowExpr {
    pub mutable: Mutable,
    pub place: Place,
    pub region: Region,
}
#[derive(Debug)]
pub struct Lambda {
    pub id: LambdaId,
    pub params: Vec<(Ident, VarId, Option<Type>)>,
    pub resource: IsResource,
    pub body: Expr,
}
#[derive(Debug)]
pub struct Place {
    pub loc: SrcLoc,
    pub kind: PlaceKind,
}
#[derive(Debug)]
pub enum PlaceKind {
    Var(Var),
    Deref(Box<Expr>),
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Builtin {
    Allocate,
    Deallocate,
    Freeze,
    BoxFromRaw,
    BoxIntoRaw,
    RefFromRaw(Mutable),
    RefIntoRaw(Mutable),
    PtrRead,
    PtrWrite,
}
impl Builtin {
    const fn _equal(b1: Builtin, b2: Builtin) -> bool {
        match (b1, b2) {
            (Builtin::Allocate, Builtin::Allocate)
            | (Builtin::Deallocate, Builtin::Deallocate)
            | (Builtin::Freeze, Builtin::Freeze)
            | (Builtin::BoxFromRaw, Builtin::BoxFromRaw)
            | (Builtin::BoxIntoRaw, Builtin::BoxIntoRaw)
            | (Builtin::PtrRead, Builtin::PtrRead)
            | (Builtin::PtrWrite, Builtin::PtrWrite) => true,
            (Builtin::RefFromRaw(mutable1), Builtin::RefFromRaw(mutable2))
            | (Builtin::RefIntoRaw(mutable1), Builtin::RefIntoRaw(mutable2)) => {
                Mutable::eq(mutable1, mutable2)
            }
            (
                Builtin::Allocate
                | Builtin::BoxFromRaw
                | Builtin::Deallocate
                | Builtin::Freeze
                | Builtin::BoxIntoRaw
                | Builtin::RefFromRaw(_)
                | Builtin::RefIntoRaw(_)
                | Builtin::PtrRead
                | Builtin::PtrWrite,
                _,
            ) => false,
        }
    }
    const _NO_REPEATS: () = {
        let mut i = 0;
        while i < Self::ALL_BUILTINS.len() {
            let mut j = 0;
            while j < Self::ALL_BUILTINS.len() {
                if i == j {
                    continue;
                }
                if Self::_equal(Self::ALL_BUILTINS[i], Self::ALL_BUILTINS[j]) {
                    panic!("repeated const")
                }
                j += 1;
            }
            i += 1;
        }
    };
    pub const COUNT: usize = 11;
    pub const ALL_BUILTINS: [Self; Self::COUNT] = [
        Builtin::Freeze,
        Builtin::Allocate,
        Builtin::Deallocate,
        Builtin::BoxFromRaw,
        Builtin::BoxIntoRaw,
        Builtin::RefFromRaw(Mutable::Immutable),
        Builtin::RefFromRaw(Mutable::Mutable),
        Builtin::RefIntoRaw(Mutable::Immutable),
        Builtin::RefIntoRaw(Mutable::Mutable),
        Builtin::PtrRead,
        Builtin::PtrWrite,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Builtin::Allocate => "allocate",
            Builtin::Deallocate => "deallocate",
            Builtin::Freeze => "freeze",
            Builtin::BoxFromRaw => "box_from_raw",
            Builtin::BoxIntoRaw => "box_into_raw",
            Builtin::RefIntoRaw(mutable) => match mutable {
                Mutable::Immutable => "ref_into_raw",
                Mutable::Mutable => "ref_into_raw_mut",
            },
            Builtin::RefFromRaw(mutable) => match mutable {
                Mutable::Immutable => "ref_from_raw",
                Mutable::Mutable => "ref_from_raw_mut",
            },
            Builtin::PtrRead => "ptr_read",
            Builtin::PtrWrite => "ptr_write",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        Self::ALL_BUILTINS
            .into_iter()
            .find(|builtin| Symbol::intern(builtin.name()) == name)
    }
    const fn index_of(self) -> usize {
        let mut i = 0;
        let builtins = Self::ALL_BUILTINS;
        let name = self.name();
        while i < builtins.len() {
            if name.eq_ignore_ascii_case(builtins[i].name()) {
                return i;
            }
            i += 1;
        }
        panic!("missing builtin")
    }
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
    pub stmts: Vec<Stmt>,
    pub expr: Box<Expr>,
}
#[derive(Debug)]
pub struct FieldInit {
    pub name: Ident,
    pub value: Expr,
}
#[derive(Debug)]
pub struct GenericArgs {
    pub loc: Option<SrcLoc>,
    pub tys: Vec<Type>,
}
impl GenericArgs {
    pub const NONE: Self = Self {
        loc: None,
        tys: Vec::new(),
    };
    pub fn tys(&self) -> Option<&[Type]> {
        if self.loc.is_some() {
            Some(&self.tys)
        } else {
            None
        }
    }
}
#[derive(Debug)]
pub enum ExprKind {
    Block(BlockBody, Option<LocalRegionId>),
    Unit,
    Err,
    Annotate(Box<Expr>, Box<Type>),
    Int(i64),
    Bool(bool),
    String(Rc<str>),
    Var(Symbol, VarId),
    Function(FunctionDefId, GenericArgs),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Borrow(Box<BorrowExpr>),
    Panic(Option<Type>),
    Lambda(Box<Lambda>),
    Deref(Box<Expr>),
    Assign(Place, Box<Expr>),
    For(Pattern, Box<Expr>, Box<Expr>),
    Case(Box<Expr>, Vec<CaseArm>),
    Print(Option<Box<Expr>>),
    List(Vec<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Record(Vec<FieldInit>),
    VariantCase(DefId, GenericArgs),
}
#[derive(Debug, Clone)]
pub enum RegionKind {
    Param(Symbol, usize),
    Local(Symbol, LocalRegionId),
    Static,
    Unknown,
}

#[derive(Debug, Clone)]
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
    Record(Vec<PatternField>),
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
    pub ty: Type,
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
    pub generics: Option<Generics>,
    pub params: Vec<Param>,
    pub return_type: Type,
    pub body: Option<Expr>,
}
#[derive(Debug)]
pub struct RecordFieldType {
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub enum TypeKind {
    Unit,
    Int,
    Bool,
    String,
    Char,
    Byte,
    Ptr(Box<Type>),
    List(Box<Type>),
    Box(Box<Type>),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
    Function(IsResource, Vec<Type>, Box<Type>),
    Named(DefId, Symbol, GenericArgs),
    Param(Symbol, usize),
    Unknown,
    Record(Vec<RecordFieldType>),
}
#[derive(Debug)]
pub struct Type {
    pub loc: SrcLoc,
    pub kind: TypeKind,
}
#[derive(Debug)]
pub struct CaseType {
    pub id: DefId,
    pub ty: Type,
}
#[derive(Debug)]
pub struct CaseDef {
    pub id: DefId,
    pub name: Ident,
    pub ty: Option<CaseType>,
}
#[derive(Debug)]
pub struct FieldDef {
    pub id: DefId,
    pub name: Ident,
    pub ty: Type,
}
#[derive(Debug)]
pub struct RecordDef {
    pub fields: IndexVec<FieldId, FieldDef>,
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
    pub generics: Option<Generics>,
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
    pub items: Vec<DefId>,
}
#[derive(Debug)]
pub enum ItemKind {
    TypeDef(TypeDef),
    Function(Function),
    Module(Module),
}
#[derive(Debug, PartialEq, Eq, Hash)]
pub enum AnnotationKind {
    Copy,
}
#[derive(Debug)]
pub struct Annotation {
    pub loc: SrcLoc,
    pub kind: AnnotationKind,
}
#[derive(Debug)]
pub struct Item {
    pub id: ItemId,
    pub annotations: Vec<Annotation>,
    pub loc: SrcLoc,
    pub kind: ItemKind,
}
impl Item {
    #[track_caller]
    pub fn expect_type_def(&self) -> &TypeDef {
        match self.kind {
            ItemKind::TypeDef(ref type_def) => type_def,
            _ => panic!("expected a type def but got {:?}", self),
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ItemId(pub DefId);
impl ItemId {
    pub fn into_def_id(self) -> DefId {
        self.0
    }
}
define_id!(DefId);
impl DefId {
    pub const ROOT: Self = Self(0);
}
#[derive(Default, Clone)]
pub struct Builtins([Option<DefId>; Builtin::COUNT], HashMap<DefId, Builtin>);
impl Builtins {
    pub fn insert(&mut self, builtin: Builtin, id: DefId) {
        let _ = self.0[builtin.index_of()].insert(id);
        self.1.insert(id, builtin);
    }
    pub fn expect_id(&self, builtin: Builtin) -> DefId {
        self.0[builtin.index_of()]
            .unwrap_or_else(|| panic!("expected builtin '{}' to be defined", builtin.name()))
    }
    pub fn builtin_for(&self, id: DefId) -> Option<Builtin> {
        self.1.get(&id).copied()
    }
}
