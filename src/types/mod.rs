use std::{
    fmt::{Debug, Display},
    ops::ControlFlow,
};

use crate::{
    Symbol,
    ast::{IsResource, Mutable},
    collect::CtxtRef,
    ident::Ident,
    index_vec::IndexVec,
    resolved_ast::{DefId, LocalRegionId},
    typed_ast::FieldId,
};
pub mod lower;
#[derive(Clone, Debug)]
pub enum PointerType {
    Box,
    Reference(Region, Mutable),
    Raw,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericKind {
    Region,
    Type,
}
#[derive(Clone, Copy, Debug)]
pub struct GenericParam {
    pub name: Ident,
    pub kind: GenericKind,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GenericArg {
    Region(Region),
    Type(Type),
}
impl GenericArg {
    pub fn expect_ty(&self) -> &Type {
        let GenericArg::Type(ty) = self else {
            unreachable!("expected a type")
        };
        ty
    }
}
impl TypeMappable for GenericArg {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        match self {
            Self::Region(region) => Ok(GenericArg::Region(region.apply_map(m)?)),
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
                    GenericArg::Region(region) => write!(f, "{}", region),
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
pub enum Region {
    Unknown,
    Static,
    Param(Symbol, usize),
    Local(Symbol, LocalRegionId),
    Infer(usize),
}
impl Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::Unknown => f.pad("{unknown}"),
            Region::Static => f.pad("static"),
            Region::Infer(_) => f.pad("_"),
            Region::Param(name, _) | Region::Local(name, _) => {
                write!(f, "{}", name)
            }
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
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum Type {
    Infer(usize),
    Unknown,
    Unit,
    Int,
    Bool,
    String,
    Char,
    Byte,
    Param(Symbol, usize),
    Box(Box<Type>),
    List(Box<Type>),
    Option(Box<Type>),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
    Function(FunctionType),
    Record(IndexVec<FieldId, RecordField>),
    RawPointer(Box<Type>),
    Array(Box<Type>, u64),
    Named(DefId, Symbol, GenericArgs),
}
impl Type {
    pub fn new_function(params: Vec<Self>, return_ty: Self) -> Self {
        Self::Function(FunctionType {
            resource: IsResource::Data,
            params,
            return_type: Box::new(return_ty),
        })
    }
    pub fn field_type(&self, field_id: FieldId) -> Option<Type> {
        match self {
            Self::Record(fields) => fields
                .iter_enumerated()
                .find(|(id, _)| *id == field_id)
                .map(|(_, field)| field.ty.clone()),
            Self::Function(FunctionType {
                resource: IsResource::Resource,
                params,
                return_type,
            }) => match field_id {
                id if id == FieldId::FIRST_FIELD => Some(Type::pointer(Type::Byte)),
                id if id == FieldId::new(1) => Some(Type::function_type(
                    IsResource::Data,
                    {
                        let mut params = params.clone();
                        params.insert(0, Self::pointer(Type::Byte));
                        params
                    },
                    (**return_type).clone(),
                )),
                _ => None,
            },
            Self::List(ty) => match field_id {
                id if id == FieldId::FIRST_FIELD => Some(Type::pointer((**ty).clone())),
                id if id == FieldId::new(1) => Some(Type::Int),
                id if id == FieldId::new(2) => Some(Type::Int),
                _ => None,
            },
            Self::String => match field_id {
                id if id == FieldId::FIRST_FIELD => Some(Type::pointer(Type::Byte)),
                id if id == FieldId::new(1) => Some(Type::Int),
                id if id == FieldId::new(2) => Some(Type::Int),
                _ => None,
            },
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
    pub fn as_option(&self) -> Option<&Type> {
        let Type::Option(ty) = self else {
            return None;
        };
        Some(ty)
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
    pub fn record(field_tys: Vec<Self>) -> Self {
        Self::Record(
            field_tys
                .into_iter()
                .enumerate()
                .map(|(i, field)| RecordField {
                    name: FieldName::Index(FieldId::new(i)),
                    ty: field,
                })
                .collect(),
        )
    }
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Imm(..) | Self::Mut(..))
    }
    pub fn reference(self, mutable: Mutable, region: Region) -> Self {
        match mutable {
            Mutable::Immutable => Self::Imm(region, Box::new(self)),
            Mutable::Mutable => Self::Mut(region, Box::new(self)),
        }
    }
    pub fn as_pointer_type(self) -> Result<(PointerType, Self), Self> {
        match self {
            Self::RawPointer(ty) => Ok((PointerType::Raw, *ty)),
            Self::Box(ty) => Ok((PointerType::Box, *ty)),
            Self::Imm(region, ty) => Ok((PointerType::Reference(region, Mutable::Immutable), *ty)),
            Self::Mut(region, ty) => Ok((PointerType::Reference(region, Mutable::Mutable), *ty)),
            _ => Err(self),
        }
    }
    pub fn pointer_type(pointer: PointerType, pointee: Self) -> Self {
        match pointer {
            PointerType::Box => Self::Box(Box::new(pointee)),
            PointerType::Reference(region, mutable) => pointee.reference(mutable, region),
            PointerType::Raw => Self::pointer(pointee),
        }
    }
    pub fn as_reference_type(&self) -> Result<(Mutable, Region, &Self), &Self> {
        let (region, mutable, ty) = match self {
            Self::Imm(region, ty) => (*region, Mutable::Immutable, ty),
            Self::Mut(region, ty) => (*region, Mutable::Mutable, ty),
            _ => return Err(self),
        };
        Ok((mutable, region, ty))
    }
    pub fn erase_regions(self) -> Self {
        struct EraseRegions;
        impl TypeMap for EraseRegions {
            type Error = std::convert::Infallible;
            fn map_region(&mut self, _: Region) -> Result<Region, Self::Error> {
                Ok(Region::Static)
            }
        }
        let Ok(ty) = EraseRegions.map_type(self);
        ty
    }
    pub fn is_resource(&self, ctxt: CtxtRef<'_>) -> bool {
        match self {
            Type::Bool
            | Type::Unit
            | Type::Unknown
            | Type::Int
            | Type::Imm(..)
            | Type::Char
            | Type::Byte
            | Type::RawPointer(_)
            | Type::Function(FunctionType {
                resource: IsResource::Data,
                ..
            }) => false,
            Type::Option(ty) | Type::Array(ty, _) => ty.is_resource(ctxt),
            Type::Mut(..)
            | Type::Function(FunctionType {
                resource: IsResource::Resource,
                ..
            })
            | Type::String
            | Type::Box(_)
            | Type::Param(..)
            | Type::List(_) => true,
            Type::Record(fields) => fields.iter().any(|field| field.ty.is_resource(ctxt)),
            Type::Infer(_) => unreachable!("Cannot 'infer' its a resource"),
            &Type::Named(..) => {
                // TODO : Add a way to mark a type as a non-resource
                true
            }
        }
    }
    pub const fn no_op_visit<T>(&self) -> ControlFlow<T> {
        ControlFlow::Continue(())
    }
    pub fn visit<T>(
        &self,
        visit_ty: &mut impl FnMut(&Self) -> ControlFlow<T>,
        visit_region: &mut impl FnMut(Region) -> ControlFlow<T>,
    ) -> ControlFlow<T> {
        visit_ty(self)?;
        match self {
            Type::Int
            | Type::Unit
            | Type::Infer(_)
            | Type::Unknown
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Byte
            | Type::Param(..)
            | Type::Box(_) => ControlFlow::Continue(()),
            Type::List(ty) | Type::Option(ty) | Type::RawPointer(ty) | Type::Array(ty, _) => {
                ty.visit(visit_ty, visit_region)
            }
            &(Type::Imm(region, ref ty) | Type::Mut(region, ref ty)) => {
                visit_region(region)?;
                ty.visit(visit_ty, visit_region)
            }
            Type::Function(function_type) => {
                for param in function_type.params.iter() {
                    param.visit(visit_ty, visit_region)?;
                }
                function_type.return_type.visit(visit_ty, visit_region)
            }
            Type::Record(fields) => {
                for field in fields {
                    visit_ty(&field.ty)?;
                }
                ControlFlow::Continue(())
            }
            Type::Named(.., generic_args) => {
                for arg in generic_args {
                    match arg {
                        &GenericArg::Region(region) => visit_region(region)?,
                        GenericArg::Type(ty) => ty.visit(visit_ty, visit_region)?,
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
            Type::Char => f.pad("char"),
            Type::Bool => f.pad("bool"),
            Type::Int => f.pad("int"),
            Type::Unit => f.pad("()"),
            Type::Unknown => f.pad("{unknown}"),
            Type::String => f.pad("string"),
            Type::Infer(_) => f.pad("_"),
            &Type::Param(name, _) => write!(f, "{}", name),
            Type::Box(ty) => {
                f.pad("box[")?;
                write!(f, "{}", ty)?;
                f.pad("]")
            }
            Type::List(ty) => {
                f.pad("list[")?;
                write!(f, "{}", ty)?;
                f.pad("]")
            }
            Type::Option(ty) => {
                f.pad("option[")?;
                write!(f, "{}", ty)?;
                f.pad("]")
            }
            Type::Imm(region, ty) => {
                write!(f, "imm [{}] {}", region, ty)
            }
            Type::Mut(region, ty) => {
                write!(f, "mut [{}] {}", region, ty)
            }
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
            | Type::Int
            | Type::Unit
            | Type::Unknown
            | Type::String
            | Type::Byte
            | Type::Infer(_)
            | Type::Param(..) => Ok(ty),
            Type::Array(ty, count) => Ok(Type::Array(Box::new(self.map_type(*ty)?), count)),
            Type::RawPointer(ty) => Ok(Type::RawPointer(Box::new(self.map_type(*ty)?))),
            Type::Box(ty) => Ok(Type::Box(Box::new(self.map_type(*ty)?))),
            Type::List(ty) => Ok(Type::List(Box::new(self.map_type(*ty)?))),
            Type::Option(ty) => Ok(Type::Option(Box::new(self.map_type(*ty)?))),
            Type::Imm(region, ty) => Ok(Type::Imm(
                self.map_region(region)?,
                Box::new(self.map_type(*ty)?),
            )),
            Type::Mut(region, ty) => Ok(Type::Mut(
                self.map_region(region)?,
                Box::new(self.map_type(*ty)?),
            )),
            Type::Function(function_type) => {
                Ok(Type::Function(self.map_function_type(function_type)?))
            }
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
                            GenericArg::Region(region) => {
                                GenericArg::Region(self.map_region(region)?)
                            }
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
    fn super_map_region(&mut self, region: Region) -> Result<Region, Self::Error> {
        Ok(region)
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
    fn map_region(&mut self, region: Region) -> Result<Region, Self::Error> {
        self.super_map_region(region)
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
impl TypeMappable for Region {
    fn apply_map<M: TypeMap + ?Sized>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_region(self)
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
