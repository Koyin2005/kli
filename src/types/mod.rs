use std::fmt::{Debug, Display};

use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    def_ids::DefId,
    define_id,
    index_vec::IndexVec,
    typed_ast::{Capture, FieldId},
};
define_id!(CaseId);
pub mod lower;
pub mod visit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TagType {
    Byte,
    Uint,
    Never,
}
impl TagType {
    pub fn into_type(self) -> Type {
        match self {
            Self::Never => Type::Never,
            Self::Byte => Type::Byte,
            Self::Uint => Type::UINT,
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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericArg(pub Type);
impl GenericArg {
    pub const fn from_type(ty: Type) -> Self {
        Self(ty)
    }
    pub fn expect_ty(&self) -> &Type {
        &self.0
    }
}
impl TypeMappable for GenericArg {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        match self {
            Self(ty) => Ok(GenericArg::from_type(ty.apply_map(m)?)),
        }
    }
}
impl std::fmt::Display for GenericArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", display_generic_args(self))
    }
}
pub fn display_generic_args<'a>(args: &'a [GenericArg]) -> DisplayGenericArgs<'a> {
    DisplayGenericArgs(args)
}
pub struct DisplayGenericArgs<'a>(&'a [GenericArg]);
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
pub struct FunctionSig {
    pub params: Vec<Type>,
    pub return_type: Type,
}
impl FunctionSig {
    pub fn new(params: Vec<Type>, return_type: Type) -> Self {
        Self {
            params,
            return_type,
        }
    }
    pub fn into_function_type(self) -> FunctionType {
        FunctionType {
            params: self.params,
            return_type: Box::new(self.return_type),
        }
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
impl FunctionType {
    pub fn new_data(params: Vec<Type>, return_type: Type) -> Self {
        Self {
            params,
            return_type: Box::new(return_type),
        }
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
pub struct GenericArgs(Vec<GenericArg>);
impl GenericArgs {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub const fn from_vec(v: Vec<GenericArg>) -> Self {
        Self(v)
    }
    pub fn from_single(arg: GenericArg) -> Self {
        Self(vec![arg])
    }
    pub fn from_type(arg: Type) -> Self {
        Self(vec![GenericArg::from_type(arg)])
    }
    pub fn combine(mut self, rest: Self) -> Self {
        self.0.extend(rest);
        self
    }
}
impl<const N: usize> TryFrom<GenericArgs> for [GenericArg; N] {
    type Error = GenericArgs;
    fn try_from(value: GenericArgs) -> Result<Self, Self::Error> {
        value.0.try_into().map_err(GenericArgs::from_vec)
    }
}
impl std::ops::Deref for GenericArgs {
    type Target = [GenericArg];
    fn deref(&self) -> GenericArgsRef<'_> {
        &self.0
    }
}
impl std::ops::DerefMut for GenericArgs {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl IntoIterator for GenericArgs {
    type IntoIter = std::vec::IntoIter<GenericArg>;
    type Item = GenericArg;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl FromIterator<GenericArg> for GenericArgs {
    fn from_iter<T: IntoIterator<Item = GenericArg>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}
pub type GenericArgsRef<'a> = &'a [GenericArg];

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct RecordField {
    pub name: FieldName,
    pub ty: Type,
}
#[derive(PartialEq, Eq, Clone, Debug, Hash, Copy)]
pub enum IntegerKind {
    Signed,
    Unsigned,
}
impl IntegerKind {
    pub const fn is_signed(self) -> bool {
        matches!(self, Self::Signed)
    }
}

#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum Type {
    Infer(usize),
    Unknown,
    Int(IntegerKind),
    Bool,
    Char,
    Byte,
    Never,
    Param(Symbol, usize),
    Function(FunctionType),
    Tuple(Vec<Type>),
    Record(IndexVec<FieldId, RecordField>),
    Array(Box<Type>),
    Named(DefId, Symbol, GenericArgs),
    String,
    Box(Box<Type>),
}
impl Type {
    pub const UNIT: Self = Self::Tuple(Vec::new());
    pub const UINT: Self = Self::Int(IntegerKind::Unsigned);
    pub const INT: Self = Self::Int(IntegerKind::Signed);
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

    pub const fn is_builtin_scalar(&self) -> bool {
        matches!(self, Self::Int(_) | Self::Bool | Self::Byte | Self::Char)
    }
    pub fn as_array(&self) -> Option<&Type> {
        let Type::Array(element) = self else {
            return None;
        };
        Some(element)
    }
    pub fn array(element: Self) -> Self {
        Self::Array(Box::new(element))
    }
    pub fn string(_: CtxtRef<'_>) -> Self {
        Type::String
    }
    pub fn as_named(&self) -> Option<(DefId, Symbol, GenericArgsRef<'_>)> {
        let Self::Named(id, name, args) = self else {
            return None;
        };
        Some((*id, *name, args))
    }
    pub fn closure_env(fields: impl Iterator<Item = Capture>) -> Self {
        Self::record_named_fields(fields.map(|capture| (capture.var.0, capture.ty)))
    }
    pub fn record_named_fields(fields: impl Iterator<Item = (Symbol, Self)>) -> Self {
        Self::Record(
            fields
                .map(|(name, ty)| RecordField {
                    name: FieldName::Named(name),
                    ty,
                })
                .collect(),
        )
    }
    pub fn new_function(params: Vec<Self>, return_ty: Self) -> Self {
        Self::Function(FunctionType {
            params,
            return_type: Box::new(return_ty),
        })
    }
    pub fn field_info(&self, field_id: FieldId, ctxt: CtxtRef<'_>) -> Option<(Type, FieldName)> {
        match self {
            Self::Record(fields) => fields
                .get(field_id)
                .map(|field| (field.ty.clone(), field.name)),
            Self::Tuple(fields) => fields
                .get(field_id.into_usize())
                .map(|ty| (ty.clone(), FieldName::Index(field_id))),
            &Self::Named(id, _, ref args) => ctxt
                .type_def(id)
                .fields()
                .get(field_id)
                .copied()
                .map(|field| (field.type_of(args, ctxt), FieldName::Named(field.name))),
            _ => None,
        }
    }
    pub fn function_type(params: Vec<Self>, return_type: Self) -> Self {
        Self::Function(FunctionType {
            params,
            return_type: Box::new(return_type),
        })
    }
    pub fn pair(first: Type, second: Type) -> Self {
        Self::tuple([first, second])
    }
    pub fn tuple(field_tys: impl IntoIterator<Item = Self>) -> Self {
        Self::Tuple(field_tys.into_iter().collect())
    }
    pub fn into_box(self, ctxt: CtxtRef<'_>) -> Result<Self, Self> {
        if self.as_box(ctxt).is_none() {
            return Err(self);
        }
        let Self::Named(_, _, args) = self else {
            return Err(self);
        };
        let [GenericArg(ty)] = args.try_into().unwrap();
        Ok(ty)
    }
    pub fn as_box(&self, ctxt: CtxtRef<'_>) -> Option<&Type> {
        use crate::lang_items::LangItem;
        let &Self::Named(id, _, ref args) = self else {
            return None;
        };
        let box_id = ctxt.lang_items().get(LangItem::Box)?;
        if id != box_id {
            return None;
        }
        Some(args.first()?.expect_ty())
    }
    pub fn is_resource(&self, ctxt: CtxtRef<'_>) -> bool {
        _ = ctxt;
        false
    }
    pub fn is_uninhabited(&self, ctxt: CtxtRef<'_>) -> bool {
        match self {
            Self::Infer(_)
            | Self::Unknown
            | Self::Int(_)
            | Self::Bool
            | Self::Char
            | Self::Byte
            | Self::Param(..)
            | Self::Function(..)
            | Self::String => false,
            Self::Never => true,
            Self::Record(fields) => fields.iter().any(|field| field.ty.is_uninhabited(ctxt)),
            Self::Tuple(fields) => fields.iter().any(|field| field.is_uninhabited(ctxt)),
            Self::Array(ty) | Self::Box(ty) => ty.is_uninhabited(ctxt),
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

impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Box(ty) => {
                write!(f, "Box[{ty}]")
            }
            Self::String => f.pad("string"),
            Type::Byte => f.pad("byte"),
            Type::Never => f.pad("never"),
            Type::Record(fields) => {
                f.pad("{")?;
                let mut first = true;
                for field in fields {
                    if !first {
                        f.pad(", ")?;
                    }
                    write!(f, "{}: {}", field.name, field.ty)?;
                    first = false;
                }
                f.pad("}")
            }
            Type::Tuple(fields) => {
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
            Type::Char => f.pad("char"),
            Type::Bool => f.pad("bool"),
            Type::Int(kind) => match kind {
                IntegerKind::Signed => f.pad("int"),
                IntegerKind::Unsigned => f.pad("uint"),
            },
            Type::Unknown => f.pad("{unknown}"),
            Type::Infer(_) => f.pad("_"),
            &Type::Param(name, _) => write!(f, "{}", name),
            Type::Function(FunctionType {
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
            Type::Named(_, name, args) => {
                write!(f, "{}{}", name, display_generic_args(args))
            }
            Type::Array(ty) => write!(f, "array[{}]", ty),
        }
    }
}
pub trait TypeMap {
    type Error;
    fn super_map_type(&mut self, ty: Type) -> Result<Type, Self::Error> {
        match ty {
            Type::String
            | Type::Bool
            | Type::Char
            | Type::Int(_)
            | Type::Unknown
            | Type::Byte
            | Type::Infer(_)
            | Type::Param(..)
            | Type::Never => Ok(ty),
            Type::Function(function_type) => {
                Ok(Type::Function(self.map_function_type(function_type)?))
            }
            Type::Tuple(fields) => Ok(Type::Tuple(
                fields
                    .into_iter()
                    .map(|field| self.map_type(field))
                    .collect::<Result<_, _>>()?,
            )),
            Type::Record(fields) => Ok(Type::Record(
                fields
                    .into_iter()
                    .map(|field| self.map_field(field))
                    .collect::<Result<_, _>>()?,
            )),
            Type::Named(id, name, args) => Ok(Type::Named(
                id,
                name,
                args.into_iter()
                    .map(|arg| arg.apply_map(self))
                    .collect::<Result<GenericArgs, _>>()?,
            )),
            Type::Array(ty) => Ok(Type::Array(Box::new(self.map_type(*ty)?))),
            Type::Box(ty) => Ok(Type::Box(Box::new(self.map_type(*ty)?))),
        }
    }
    fn super_map_function_type(
        &mut self,
        mut function_type: FunctionType,
    ) -> Result<FunctionType, Self::Error> {
        function_type.params = function_type
            .params
            .into_iter()
            .map(|param| self.map_type(param))
            .collect::<Result<_, _>>()?;
        *function_type.return_type = self.map_type(*function_type.return_type)?;
        Ok(function_type)
    }
    fn super_map_field(&mut self, field: RecordField) -> Result<RecordField, Self::Error> {
        let mut field = field;
        let ty = self.map_type(field.ty)?;
        field.ty = ty;
        Ok(field)
    }
    fn map_type(&mut self, ty: Type) -> Result<Type, Self::Error> {
        self.super_map_type(ty)
    }
    fn map_field(&mut self, field: RecordField) -> Result<RecordField, Self::Error> {
        self.super_map_field(field)
    }
    fn map_function_type(
        &mut self,
        function_type: FunctionType,
    ) -> Result<FunctionType, Self::Error> {
        self.super_map_function_type(function_type)
    }
}

pub trait TypeMappable {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error>
    where
        Self: Sized;
}

impl TypeMappable for Type {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_type(self)
    }
}

impl TypeMappable for FunctionType {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_function_type(self)
    }
}
impl TypeMappable for RecordField {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_field(self)
    }
}
impl TypeMappable for FunctionSig {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error>
    where
        Self: Sized,
    {
        Ok(Self {
            params: self
                .params
                .into_iter()
                .map(|param| m.map_type(param))
                .collect::<Result<_, _>>()?,
            return_type: m.map_type(self.return_type)?,
        })
    }
}
impl<T: TypeMappable> TypeMappable for Box<T> {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        Ok(Box::new((*self).apply_map(m)?))
    }
}

impl TypeMappable for GenericArgs {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error>
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
