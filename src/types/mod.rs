use std::{
    fmt::{Debug, Display},
    ops::ControlFlow,
};

use crate::{
    Symbol,
    ast::IsResource,
    collect::{CtxtRef, TypeDefKind},
    def_ids::DefId,
    define_id,
    index_vec::IndexVec,
    lang_items::LangItem,
    typed_ast::{Capture, FieldId},
};
define_id!(CaseId);
pub mod lower;
#[derive(Clone, Debug)]
pub enum PointerType {
    Box,
    Raw,
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
pub enum GenericArg {
    Type(Type),
}
impl GenericArg {
    pub fn expect_ty(&self) -> &Type {
        let GenericArg::Type(ty) = self;
        ty
    }
}
impl TypeMappable for GenericArg {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        match self {
            Self::Type(ty) => Ok(GenericArg::Type(ty.apply_map(m)?)),
        }
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
                match arg {
                    GenericArg::Type(ty) => write!(f, "{}", ty),
                }?;
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
            resource: IsResource::Data,
            params: self.params,
            return_type: Box::new(self.return_type),
        }
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct FunctionType {
    pub resource: IsResource,
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
impl FunctionType {
    pub fn new_data(params: Vec<Type>, return_type: Type) -> Self {
        Self {
            resource: IsResource::Data,
            params,
            return_type: Box::new(return_type),
        }
    }
    pub fn new_resource(params: Vec<Type>, return_type: Type) -> Self {
        Self {
            resource: IsResource::Resource,
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
pub type GenericArgs = Vec<GenericArg>;
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
    RawPointer(Box<Type>),
    Array(Box<Type>, u64),
    Named(DefId, Symbol, GenericArgs),
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
        matches!(
            self,
            Self::Int(_) | Self::Bool | Self::Byte | Self::Char | Self::RawPointer(_)
        )
    }
    pub fn string(ctxt: CtxtRef<'_>) -> Self {
        let id = ctxt.lang_items().expect(LangItem::String);
        let name = ctxt.expect_ident(id).symbol;
        Type::Named(id, name, GenericArgs::new())
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
            resource: IsResource::Data,
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
            Self::Function(FunctionType {
                resource: IsResource::Resource,
                params,
                return_type,
            }) => match field_id {
                id if id == FieldId::FIRST_FIELD => Some((
                    Type::pointer(Type::Byte),
                    FieldName::Named(Symbol::intern("env")),
                )),
                id if id == FieldId::new(1) => Some((
                    Type::function_type(
                        IsResource::Data,
                        {
                            let mut params = params.clone();
                            params.insert(0, Self::pointer(Type::Byte));
                            params
                        },
                        (**return_type).clone(),
                    ),
                    FieldName::Named(Symbol::intern("code")),
                )),
                _ => None,
            },
            &Self::Named(id, _, ref args) => ctxt
                .type_def(id)
                .fields()
                .get(field_id)
                .copied()
                .map(|field| (field.type_of(args, ctxt), FieldName::Named(field.name))),
            _ => None,
        }
    }
    pub fn function_type(resource: IsResource, params: Vec<Self>, return_type: Self) -> Self {
        Self::Function(FunctionType {
            resource,
            params,
            return_type: Box::new(return_type),
        })
    }
    pub fn as_pointer(&self) -> Option<&Type> {
        let Type::RawPointer(ty) = self else {
            return None;
        };
        Some(ty)
    }
    pub fn pointer(ty: Self) -> Self {
        Self::RawPointer(Box::new(ty))
    }
    pub fn pair(first: Type, second: Type) -> Self {
        Self::tuple([first, second])
    }
    pub fn tuple(field_tys: impl IntoIterator<Item = Self>) -> Self {
        Self::Tuple(field_tys.into_iter().collect())
    }
    pub fn into_pointer_type(self, ctxt: CtxtRef<'_>) -> Result<(PointerType, Self), Self> {
        match self {
            Self::RawPointer(ty) => Ok((PointerType::Raw, *ty)),
            Self::Named(..) => Ok((PointerType::Box, self.into_box(ctxt)?)),
            _ => Err(self),
        }
    }
    pub fn into_box(self, ctxt: CtxtRef<'_>) -> Result<Self, Self> {
        if self.as_box(ctxt).is_none() {
            return Err(self);
        }
        let Self::Named(_, _, args) = self else {
            return Err(self);
        };
        let [arg] = args.try_into().unwrap();
        let GenericArg::Type(ty) = arg;
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
        let arg = args.first()?;
        let GenericArg::Type(ty) = arg;
        Some(ty)
    }
    pub fn pointer_kind(&self, ctxt: CtxtRef<'_>) -> Option<PointerType> {
        match self {
            Self::RawPointer(_) => Some(PointerType::Raw),
            Self::Named(..) if self.as_box(ctxt).is_some() => Some(PointerType::Box),
            _ => None,
        }
    }
    pub fn pointer_type(pointer: PointerType, pointee: Self, ctxt: CtxtRef<'_>) -> Self {
        match pointer {
            PointerType::Box => {
                let id = ctxt.lang_items().expect(LangItem::Box);
                let name = ctxt.expect_ident(id).symbol;
                Self::Named(id, name, vec![GenericArg::Type(pointee)])
            }
            PointerType::Raw => Self::pointer(pointee),
        }
    }
    pub fn is_resource(&self, ctxt: CtxtRef<'_>) -> bool {
        _ = ctxt;
        false
    }
    pub const fn no_op_visit<T>(&self) -> ControlFlow<T> {
        ControlFlow::Continue(())
    }
    pub fn is_uninhabited(&self, ctxt: CtxtRef<'_>) -> bool {
        match self {
            Type::Infer(_)
            | Type::Unknown
            | Type::Int(_)
            | Type::Bool
            | Type::Char
            | Type::Byte
            | Type::Param(..)
            | Type::Function(..) => false,
            Type::Never => true,
            Type::Record(fields) => fields.iter().any(|field| field.ty.is_uninhabited(ctxt)),
            Type::Tuple(fields) => fields.iter().any(|field| field.is_uninhabited(ctxt)),
            Type::RawPointer(_) => false,
            Type::Array(ty, _) => ty.is_uninhabited(ctxt),
            Type::Named(def_id, _, generic_args) => {
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
    pub fn visit<T>(&self, visit_ty: &mut impl FnMut(&Self) -> ControlFlow<T>) -> ControlFlow<T> {
        visit_ty(self)?;
        match self {
            Type::Int(_)
            | Type::Infer(_)
            | Type::Unknown
            | Type::Bool
            | Type::Char
            | Type::Byte
            | Type::Param(..)
            | Type::Never => ControlFlow::Continue(()),
            Type::RawPointer(ty) | Type::Array(ty, _) => ty.visit(visit_ty),
            Type::Function(function_type) => {
                for param in function_type.params.iter() {
                    param.visit(visit_ty)?;
                }
                function_type.return_type.visit(visit_ty)
            }
            Type::Record(fields) => {
                for field in fields {
                    visit_ty(&field.ty)?;
                }
                ControlFlow::Continue(())
            }
            Type::Tuple(fields) => {
                for field in fields {
                    visit_ty(field)?;
                }
                ControlFlow::Continue(())
            }
            Type::Named(.., generic_args) => {
                for arg in generic_args {
                    match arg {
                        GenericArg::Type(ty) => ty.visit(visit_ty)?,
                    }
                }
                ControlFlow::Continue(())
            }
        }
    }
}

impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Array(ty, count) => {
                write!(f, "fixed_array[{},{}]", ty, count)
            }
            Type::Byte => f.pad("byte"),
            Type::RawPointer(ty) => {
                write!(f, "ptr[{}]", ty)
            }
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
                resource,
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
                f.pad(match *resource {
                    IsResource::Data => ") -> ",
                    IsResource::Resource => ") => ",
                })?;
                write!(f, "{}", return_type)
            }
            Type::Named(_, name, args) => {
                write!(f, "{}{}", name, display_generic_args(args))
            }
        }
    }
}
pub trait TypeMap {
    type Error;
    fn super_map_type(&mut self, ty: Type) -> Result<Type, Self::Error> {
        match ty {
            Type::Bool
            | Type::Char
            | Type::Int(_)
            | Type::Unknown
            | Type::Byte
            | Type::Infer(_)
            | Type::Param(..)
            | Type::Never => Ok(ty),
            Type::Array(ty, count) => Ok(Type::Array(Box::new(self.map_type(*ty)?), count)),
            Type::RawPointer(ty) => Ok(Type::RawPointer(Box::new(self.map_type(*ty)?))),
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
                    .map(|arg| {
                        Ok(match arg {
                            GenericArg::Type(ty) => GenericArg::Type(self.map_type(ty)?),
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
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

pub const LIST_PTR_FIELD: FieldId = FieldId::new(0);
pub const LIST_CAPICITY_FIELD: FieldId = FieldId::new(1);
pub const LIST_LEN_FIELD: FieldId = FieldId::new(2);
