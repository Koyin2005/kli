use std::fmt::{Debug, Display};

use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    def_ids::DefId,
    define_id,
    typed_ast::FieldId,
};
define_id!(CaseId);
pub mod lower;
pub mod visit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TagType {
    UInt8,
    Uint64,
    Never,
}
impl TagType {
    pub fn into_type<'ctxt>(self, ctxt: CtxtRef<'ctxt>) -> Type<'ctxt> {
        match self {
            Self::Never => Type::new_never(ctxt),
            Self::UInt8 => Type::new_uint(ctxt, IntegerSize::Int8),
            Self::Uint64 => Type::new_uint(ctxt, IntegerSize::Int64),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericKind {
    Type,
}
#[derive(Clone, Copy, Debug)]
pub struct GenericParam {
    pub name: Symbol,
    pub kind: GenericKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub struct GenericArg<'ctxt>(pub Type<'ctxt>);
impl<'ctxt> GenericArg<'ctxt> {
    pub const fn from_type(ty: Type<'ctxt>) -> Self {
        Self(ty)
    }
    pub fn expect_ty(self) -> Type<'ctxt> {
        self.0
    }
}
impl<'ctxt> TypeMappable<'ctxt> for GenericArg<'ctxt> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        match self {
            Self(ty) => Ok(GenericArg::from_type(ty.apply_map(m)?)),
        }
    }
}
impl std::fmt::Display for GenericArgs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", display_generic_args(self))
    }
}
fn display_generic_args<'a>(args: &'a [GenericArg]) -> DisplayGenericArgs<'a> {
    DisplayGenericArgs(args)
}
pub struct DisplayGenericArgs<'a>(&'a [GenericArg<'a>]);
impl Display for DisplayGenericArgs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            Ok(())
        } else {
            write!(f, "[")?;
            let mut first = true;
            for arg in self.0 {
                if !first {
                    write!(f, ",")?;
                }
                write!(f, "{}", arg.0)?;
                first = false;
            }
            write!(f, "]")
        }
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct FunctionSig<'ctxt> {
    pub params: Vec<Type<'ctxt>>,
    pub return_type: Type<'ctxt>,
}
impl<'ctxt> FunctionSig<'ctxt> {
    pub const fn new(params: Vec<Type<'ctxt>>, return_type: Type<'ctxt>) -> Self {
        Self {
            params,
            return_type,
        }
    }
    pub fn into_type(self, ctxt: CtxtRef<'ctxt>) -> Type<'ctxt> {
        TypeKind::Function(self).intern(ctxt)
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash, Copy)]
pub enum FieldName {
    Named(Symbol),
    Index(FieldId),
}

impl Display for FieldName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldName::Index(index) => write!(f, "{}", index.into_usize()),
            FieldName::Named(name) => {
                write!(f, "{}", name)
            }
        }
    }
}
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct GenericArgs<'ctxt>(Vec<GenericArg<'ctxt>>);
impl<'ctxt> GenericArgs<'ctxt> {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub const fn from_vec(v: Vec<GenericArg<'ctxt>>) -> Self {
        Self(v)
    }
    pub fn from_single(arg: GenericArg<'ctxt>) -> Self {
        Self(vec![arg])
    }
    pub fn from_type(arg: Type<'ctxt>) -> Self {
        Self(vec![GenericArg::from_type(arg)])
    }
    pub fn combine(mut self, rest: Self) -> Self {
        self.0.extend(rest);
        self
    }
}
impl<'ctxt, const N: usize> TryFrom<GenericArgs<'ctxt>> for [GenericArg<'ctxt>; N] {
    type Error = GenericArgs<'ctxt>;
    fn try_from(value: GenericArgs<'ctxt>) -> Result<Self, Self::Error> {
        value.0.try_into().map_err(GenericArgs::from_vec)
    }
}
impl<'ctxt> std::ops::Deref for GenericArgs<'ctxt> {
    type Target = [GenericArg<'ctxt>];
    fn deref(&self) -> GenericArgsRef<'_, 'ctxt> {
        &self.0
    }
}
impl<'ctxt> std::ops::DerefMut for GenericArgs<'ctxt> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl<'ctxt> IntoIterator for GenericArgs<'ctxt> {
    type IntoIter = std::vec::IntoIter<GenericArg<'ctxt>>;
    type Item = GenericArg<'ctxt>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'ctxt> FromIterator<GenericArg<'ctxt>> for GenericArgs<'ctxt> {
    fn from_iter<T: IntoIterator<Item = GenericArg<'ctxt>>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
pub type GenericArgsRef<'a, 'b> = &'a [GenericArg<'b>];

#[derive(PartialEq, Eq, Clone, Debug, Hash, Copy)]
pub struct RecordField<'ctxt> {
    pub name: FieldName,
    pub ty: Type<'ctxt>,
}

#[derive(PartialEq, Eq, Clone, Debug, Hash, Copy)]
pub enum IntegerSize {
    Int8,
    Int32,
    Int64,
}
impl IntegerSize {
    pub const fn bit_width(self) -> u8 {
        match self {
            Self::Int8 => 8,
            Self::Int32 => 32,
            Self::Int64 => 64,
        }
    }
    pub const fn is_byte_sized(self) -> bool {
        matches!(self, IntegerSize::Int8)
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash, Copy)]
pub enum IntegerKind {
    Signed(IntegerSize),
    Unsigned(IntegerSize),
}
impl IntegerKind {
    pub const UINT8: Self = IntegerKind::Unsigned(IntegerSize::Int8);
    pub const UINT32: Self = IntegerKind::Unsigned(IntegerSize::Int32);

    pub fn name_str(self) -> &'static str {
        match self {
            IntegerKind::Signed(IntegerSize::Int8) => "Int8",
            IntegerKind::Signed(IntegerSize::Int32) => "Int32",
            IntegerKind::Signed(IntegerSize::Int64) => "Int64",
            IntegerKind::Unsigned(IntegerSize::Int8) => "UInt8",
            IntegerKind::Unsigned(IntegerSize::Int32) => "UInt32",
            IntegerKind::Unsigned(IntegerSize::Int64) => "UInt64",
        }
    }
    pub const fn min_value_scalar(self) -> i128 {
        match self {
            Self::Signed(IntegerSize::Int8) => i8::MIN as i128,
            Self::Unsigned(IntegerSize::Int8) => u8::MIN as i128,
            Self::Signed(IntegerSize::Int32) => i32::MIN as i128,
            Self::Unsigned(IntegerSize::Int32) => u32::MIN as i128,
            Self::Signed(IntegerSize::Int64) => i64::MIN as i128,
            Self::Unsigned(IntegerSize::Int64) => u64::MIN as i128,
        }
    }
    pub const fn max_value_scalar(self) -> i128 {
        match self {
            Self::Signed(size) => match size {
                IntegerSize::Int64 => i64::MAX as i128,
                IntegerSize::Int8 => i8::MAX as i128,
                IntegerSize::Int32 => i32::MAX as i128,
            },
            Self::Unsigned(size) => match size {
                IntegerSize::Int8 => u8::MAX as i128,
                IntegerSize::Int64 => u64::MAX as i128,
                IntegerSize::Int32 => u32::MAX as i128,
            },
        }
    }
    pub const fn size(self) -> IntegerSize {
        let (Self::Signed(size) | Self::Unsigned(size)) = self;
        size
    }
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::Signed(_))
    }
    pub const fn signed_and_size(self) -> (bool, IntegerSize) {
        match self {
            Self::Signed(size) => (true, size),
            Self::Unsigned(size) => (false, size),
        }
    }
}
impl Display for IntegerKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(self.name_str())
    }
}
#[derive(PartialEq, Eq, Clone, Copy, Hash)]
pub struct Type<'ctxt>(&'ctxt TypeKind<'ctxt>);
impl<'ctxt> Type<'ctxt> {
    pub const UNKNOWN: Self = Self(&TypeKind::Unknown);
    pub const BYTE: Self = Self(&TypeKind::Unknown);
    pub fn new_uninit(ctxt: CtxtRef<'ctxt>, ty: Self) -> Self {
        TypeKind::Uninit(ty).intern(ctxt)
    }
    pub fn as_uninit(self) -> Option<Self> {
        let &TypeKind::Uninit(ty) = self.0 else {
            return None;
        };
        Some(ty)
    }
    pub fn new_integer(ctxt: CtxtRef<'ctxt>, kind: IntegerKind) -> Self {
        TypeKind::Int(kind).intern(ctxt)
    }
    pub fn new_integer_var(ctxt: CtxtRef<'ctxt>, var: usize) -> Self {
        TypeKind::IntVar(var).intern(ctxt)
    }
    pub fn new_int(ctxt: CtxtRef<'ctxt>, size: IntegerSize) -> Self {
        Self::new_integer(ctxt, IntegerKind::Signed(size))
    }
    pub fn new_bool(ctxt: CtxtRef<'ctxt>) -> Self {
        TypeKind::Bool.intern(ctxt)
    }
    pub fn new_uint(ctxt: CtxtRef<'ctxt>, size: IntegerSize) -> Self {
        Self::new_integer(ctxt, IntegerKind::Unsigned(size))
    }
    pub fn new_char(ctxt: CtxtRef<'ctxt>) -> Self {
        TypeKind::Char.intern(ctxt)
    }
    pub fn new_never(ctxt: CtxtRef<'ctxt>) -> Self {
        TypeKind::Never.intern(ctxt)
    }
    pub fn new_unknown(ctxt: CtxtRef<'ctxt>) -> Self {
        TypeKind::Unknown.intern(ctxt)
    }
    pub fn new_unit(ctxt: CtxtRef<'ctxt>) -> Self {
        TypeKind::UNIT.intern(ctxt)
    }
    pub fn is_bool(self) -> bool {
        matches!(self.0, TypeKind::Bool)
    }
    pub fn is_char(self) -> bool {
        matches!(self.0, TypeKind::Char)
    }

    pub fn new(kind: &'ctxt TypeKind) -> Self {
        Self(kind)
    }
    pub fn kind(&self) -> &'_ TypeKind<'ctxt> {
        self.0
    }
    pub fn named(
        ctxt: CtxtRef<'ctxt>,
        id: DefId,
        name: Symbol,
        args: impl IntoIterator<Item = GenericArg<'ctxt>>,
    ) -> Self {
        TypeKind::Named(id, name, args.into_iter().collect()).intern(ctxt)
    }
    pub fn param(ctxt: CtxtRef<'ctxt>, name: Symbol, index: usize) -> Self {
        TypeKind::Param(name, index).intern(ctxt)
    }
    pub fn infer_var(ctxt: CtxtRef<'ctxt>, var: usize) -> Self {
        TypeKind::Infer(var).intern(ctxt)
    }
    pub fn function_type(ctxt: CtxtRef<'ctxt>, params: Vec<Self>, return_type: Self) -> Self {
        FunctionSig::new(params, return_type).into_type(ctxt)
    }
    pub fn new_box(ctxt: CtxtRef<'ctxt>, ty: Self) -> Self {
        TypeKind::Box(ty).intern(ctxt)
    }
    pub fn as_box(self) -> Option<Self> {
        let &TypeKind::Box(ty) = self.kind() else {
            return None;
        };
        Some(ty)
    }
    pub fn new_raw_array(ctxt: CtxtRef<'ctxt>, ty: Self) -> Self {
        Self::new_array(ctxt, Self::new_uninit(ctxt, ty))
    }
    pub fn as_raw_array(self) -> Option<Self> {
        self.as_array()?.as_uninit()
    }
    pub fn new_array(ctxt: CtxtRef<'ctxt>, ty: Self) -> Self {
        TypeKind::Array(ty).intern(ctxt)
    }
    pub fn new_string(ctxt: CtxtRef<'ctxt>) -> Self {
        TypeKind::String.intern(ctxt)
    }

    pub fn as_array(self) -> Option<Type<'ctxt>> {
        let TypeKind::Array(element) = self.0 else {
            return None;
        };
        Some(*element)
    }
    pub fn tuple_from_iter(
        ctxt: CtxtRef<'ctxt>,
        field_tys: impl IntoIterator<Item = Self>,
    ) -> Self {
        TypeKind::Tuple(field_tys.into_iter().collect()).intern(ctxt)
    }
    pub fn pair(ctxt: CtxtRef<'ctxt>, first: Self, second: Self) -> Self {
        Self::tuple_from_iter(ctxt, [first, second])
    }
    pub fn as_tuple(self) -> Option<&'ctxt Vec<Self>> {
        let TypeKind::Tuple(fields) = self.0 else {
            return None;
        };
        Some(fields)
    }
    pub fn into_box(self) -> Result<Self, Self> {
        let &TypeKind::Box(ty) = self.kind() else {
            return Err(self);
        };
        Ok(ty)
    }
    pub const fn as_integer(self) -> Option<IntegerKind> {
        let &TypeKind::Int(kind) = self.0 else {
            return None;
        };
        Some(kind)
    }
    pub const fn is_integer(self) -> bool {
        matches!(self.0, TypeKind::Int(_))
    }
    pub fn is_uint(self, size: IntegerSize) -> bool {
        self.is_integer_kind(IntegerKind::Unsigned(size))
    }
    pub fn is_integer_kind(self, kind: IntegerKind) -> bool {
        let &TypeKind::Int(int_kind) = self.0 else {
            return false;
        };
        int_kind == kind
    }
    pub fn as_function(self) -> Option<&'ctxt FunctionSig<'ctxt>> {
        let TypeKind::Function(function) = self.0 else {
            return None;
        };
        Some(function)
    }
}
impl<'ctxt> std::ops::Deref for Type<'ctxt> {
    type Target = TypeKind<'ctxt>;
    fn deref(&self) -> &Self::Target {
        self.kind()
    }
}
impl std::fmt::Debug for Type<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}
impl std::fmt::Display for Type<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum TypeKind<'ctxt> {
    Infer(usize),
    Unknown,
    Int(IntegerKind),
    IntVar(usize),
    Bool,
    Char,
    Never,
    Param(Symbol, usize),
    Function(FunctionSig<'ctxt>),
    Tuple(Vec<Type<'ctxt>>),
    Array(Type<'ctxt>),
    Named(DefId, Symbol, GenericArgs<'ctxt>),
    String,
    Box(Type<'ctxt>),
    Uninit(Type<'ctxt>),
}
impl<'ctxt> TypeKind<'ctxt> {
    pub const UNIT: Self = Self::Tuple(Vec::new());
    pub fn intern(self, ctxt: CtxtRef<'ctxt>) -> Type<'ctxt> {
        ctxt.intern_ty(self)
    }
    pub const fn is_unit(&self) -> bool {
        match self {
            Self::Tuple(fields) => fields.is_empty(),
            _ => false,
        }
    }
    pub fn is_bool(&self) -> bool {
        matches!(self, Self::Bool)
    }
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::Int(_))
    }
    pub fn is_integer_kind(&self, kind: IntegerKind) -> bool {
        matches!(self, Self::Int(int_kind) if kind == *int_kind)
    }

    pub const fn is_builtin_scalar(&self) -> bool {
        matches!(self, Self::Int(_) | Self::Bool | Self::Char)
    }
    pub fn array(element: Type<'ctxt>) -> Self {
        Self::Array(element)
    }
    pub fn string(_: CtxtRef<'_>) -> Self {
        TypeKind::String
    }
    pub fn as_named(&self) -> Option<(DefId, Symbol, GenericArgsRef<'_, 'ctxt>)> {
        let Self::Named(id, name, args) = self else {
            return None;
        };
        Some((*id, *name, args))
    }
    pub fn field_info(
        &self,
        field_id: FieldId,
        ctxt: CtxtRef<'ctxt>,
    ) -> Option<(Type<'ctxt>, FieldName)> {
        match self {
            Self::Tuple(fields) => fields
                .get(field_id.into_usize())
                .map(|ty| (*ty, FieldName::Index(field_id))),
            &Self::Named(id, _, ref args) => ctxt
                .type_def(id)
                .fields()
                .get(field_id)
                .copied()
                .map(|field| (field.type_of(args, ctxt), FieldName::Named(field.name))),
            _ => None,
        }
    }
    pub fn is_resource(&self, ctxt: CtxtRef<'_>) -> bool {
        _ = ctxt;
        false
    }
    pub fn is_uninhabited(&self, ctxt: CtxtRef<'ctxt>) -> bool {
        match self {
            Self::Infer(_)
            | Self::Unknown
            | Self::Int(_)
            | Self::Bool
            | Self::Char
            | Self::Param(..)
            | Self::Function(..)
            | Self::String
            | Self::Array(_)
            | Self::Uninit(_)
            | Self::IntVar(_) => false,
            Self::Never => true,
            Self::Tuple(fields) => fields.iter().any(|field| field.is_uninhabited(ctxt)),
            Self::Box(ty) => ty.is_uninhabited(ctxt),
            Self::Named(def_id, _, generic_args) => {
                if ctxt.is_type_recursive(*def_id) {
                    false
                } else {
                    match ctxt.type_def(*def_id).kind {
                        TypeDefKind::Record(ref fields) => fields
                            .iter()
                            .any(|field| field.type_of(generic_args, ctxt).is_uninhabited(ctxt)),
                        TypeDefKind::Variant(ref cases) => cases.iter().all(|case| {
                            case.field.is_some_and(|field| {
                                field.type_of(generic_args, ctxt).is_uninhabited(ctxt)
                            })
                        }),
                    }
                }
            }
        }
    }
}

impl Display for TypeKind<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Box(ty) => {
                write!(f, "Box[{ty}]")
            }
            Self::String => f.pad("string"),
            TypeKind::Never => f.pad("never"),
            TypeKind::Tuple(fields) => {
                f.pad("(")?;
                let mut first = true;
                for field in fields {
                    if !first {
                        f.pad(", ")?;
                    }
                    write!(f, "{}", field)?;
                    if first && fields.len() == 1 {
                        f.pad(",")?;
                    }
                    first = false;
                }
                f.pad(")")
            }
            TypeKind::Char => f.pad("char"),
            TypeKind::Bool => f.pad("bool"),
            TypeKind::Int(kind) => write!(f, "{}", kind),
            TypeKind::Unknown => f.pad("{unknown}"),
            TypeKind::Infer(_) => f.pad("_"),
            TypeKind::IntVar(_) => f.pad("{integer}"),
            &TypeKind::Param(name, _) => write!(f, "{}", name),
            TypeKind::Function(FunctionSig {
                params,
                return_type,
            }) => {
                f.pad("fun(")?;
                let mut first = true;
                for param in params {
                    if !first {
                        f.pad(",")?;
                    }
                    write!(f, "{}", param)?;
                    first = false;
                }
                write!(f, ") -> {}", return_type)
            }
            TypeKind::Named(_, name, args) => {
                write!(f, "{}{}", name, display_generic_args(args))
            }
            TypeKind::Array(ty) => write!(f, "array[{}]", ty),
            TypeKind::Uninit(ty) => write!(f, "uninit[{}]", ty),
        }
    }
}
pub trait TypeMap<'ctxt> {
    type Error;
    fn ctxt(&self) -> CtxtRef<'ctxt>;
    fn super_map_type(&mut self, ty: Type<'ctxt>) -> Result<Type<'ctxt>, Self::Error> {
        match ty.kind() {
            TypeKind::String
            | TypeKind::Bool
            | TypeKind::Char
            | TypeKind::Int(_)
            | TypeKind::Unknown
            | TypeKind::Infer(_)
            | TypeKind::Param(..)
            | TypeKind::IntVar(_)
            | TypeKind::Never => Ok(ty),
            TypeKind::Function(function_type) => Ok(self
                .map_function_type(function_type.clone())?
                .into_type(self.ctxt())),
            TypeKind::Tuple(fields) => Ok(Type::tuple_from_iter(self.ctxt(), {
                let fields: Vec<_> = fields
                    .iter()
                    .map(|&field| self.map_type(field))
                    .collect::<Result<_, _>>()?;
                fields
            })),
            &TypeKind::Named(id, name, ref args) => Ok(Type::named(
                self.ctxt(),
                id,
                name,
                args.iter()
                    .map(|&arg| arg.apply_map(self))
                    .collect::<Result<GenericArgs, _>>()?,
            )),
            TypeKind::Array(ty) => Ok(Type::new_array(self.ctxt(), self.map_type(*ty)?)),
            TypeKind::Box(ty) => Ok(Type::new_box(self.ctxt(), self.map_type(*ty)?)),
            TypeKind::Uninit(ty) => Ok(Type::new_uninit(self.ctxt(), self.map_type(*ty)?)),
        }
    }
    fn super_map_function_type(
        &mut self,
        mut function_type: FunctionSig<'ctxt>,
    ) -> Result<FunctionSig<'ctxt>, Self::Error> {
        function_type.params = function_type
            .params
            .into_iter()
            .map(|param| self.map_type(param))
            .collect::<Result<_, _>>()?;
        function_type.return_type = self.map_type(function_type.return_type)?;
        Ok(function_type)
    }
    fn super_map_field(
        &mut self,
        field: RecordField<'ctxt>,
    ) -> Result<RecordField<'ctxt>, Self::Error> {
        let mut field = field;
        let ty = self.map_type(field.ty)?;
        field.ty = ty;
        Ok(field)
    }
    fn map_type(&mut self, ty: Type<'ctxt>) -> Result<Type<'ctxt>, Self::Error> {
        self.super_map_type(ty)
    }
    fn map_field(&mut self, field: RecordField<'ctxt>) -> Result<RecordField<'ctxt>, Self::Error> {
        self.super_map_field(field)
    }
    fn map_function_type(
        &mut self,
        function_type: FunctionSig<'ctxt>,
    ) -> Result<FunctionSig<'ctxt>, Self::Error> {
        self.super_map_function_type(function_type)
    }
}

pub trait TypeMappable<'ctxt> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error>
    where
        Self: Sized;
}

impl<'ctxt> TypeMappable<'ctxt> for Type<'ctxt> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_type(self)
    }
}

impl<'ctxt> TypeMappable<'ctxt> for FunctionSig<'ctxt> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_function_type(self)
    }
}
impl<'ctxt> TypeMappable<'ctxt> for RecordField<'ctxt> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_field(self)
    }
}
impl<'ctxt, T: TypeMappable<'ctxt>> TypeMappable<'ctxt> for Box<T> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        Ok(Box::new((*self).apply_map(m)?))
    }
}

impl<'ctxt> TypeMappable<'ctxt> for GenericArgs<'ctxt> {
    fn apply_map<M: TypeMap<'ctxt> + ?Sized>(self, m: &mut M) -> Result<Self, M::Error>
    where
        Self: Sized,
    {
        Ok(GenericArgs(
            self.0
                .into_iter()
                .map(|arg| arg.apply_map(m))
                .collect::<Result<Vec<_>, _>>()?,
        ))
    }
}
pub const LIST_PTR_FIELD: FieldId = FieldId::new(0);
pub const LIST_CAPICITY_FIELD: FieldId = FieldId::new(1);
pub const LIST_LEN_FIELD: FieldId = FieldId::new(2);
