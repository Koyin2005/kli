use std::{fmt::Display, rc::Rc};

use crate::{
    ast::{IsResource, Mutable},
    index_vec::IndexVec,
    resolved_ast::LocalRegionId,
    typed_ast::FieldId,
};
#[derive(Clone, Copy, Debug)]
pub enum GenericKind {
    Region,
    Type,
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GenericArg {
    Region(Region),
    Type(Type),
}
impl TypeMappable for GenericArg {
    fn apply_map<M: TypeMap>(self, m: &mut M) -> Result<Self, M::Error>
    where
        Self: Sized,
    {
        match self {
            Self::Region(region) => Ok(GenericArg::Region(region.apply_map(m)?)),
            Self::Type(ty) => Ok(GenericArg::Type(ty.apply_map(m)?)),
        }
    }
}
pub struct DisplayGenericArgs<'a>(pub &'a [GenericArg]);
impl Display for DisplayGenericArgs<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub enum Region {
    Unknown,
    Static,
    Param(Rc<str>, usize),
    Local(Rc<str>, LocalRegionId),
    Infer(usize),
}
impl Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => f.pad("{unknown}"),
            Self::Static => f.pad("static"),
            Self::Infer(_) => f.pad("_"),
            Self::Param(name, _) | Self::Local(name, _) => f.pad(name),
        }
    }
}
#[derive(PartialEq, Eq, Clone, Debug, Hash)]
pub struct RecordField {
    pub name: Rc<str>,
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
    Param(Rc<str>, usize),
    Box(Box<Type>),
    List(Box<Type>),
    Option(Box<Type>),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
    Function(FunctionType),
    Record(IndexVec<FieldId, RecordField>),
    RawPointer,
}
impl Type {
    pub fn record(field_tys: Vec<Self>) -> Self {
        Self::Record(
            field_tys
                .into_iter()
                .enumerate()
                .map(|(i, field)| RecordField {
                    name: Rc::from(i.to_string()),
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
    pub fn as_reference_type(&self) -> Result<(Mutable, &Region, &Self), &Self> {
        let (region, mutable, ty) = match self {
            Self::Imm(region, ty) => (region, Mutable::Immutable, ty),
            Self::Mut(region, ty) => (region, Mutable::Mutable, ty),
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
}
impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RawPointer => f.write_str("ptr"),
            Self::Record(fields) => {
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
            Self::Char => f.pad("char"),
            Self::Bool => f.pad("bool"),
            Self::Int => f.pad("int"),
            Self::Unit => f.pad("()"),
            Self::Unknown => f.pad("{unknown}"),
            Self::String => f.pad("string"),
            Self::Infer(_) => f.pad("_"),
            Self::Param(name, _) => f.pad(name),
            Self::Box(ty) => {
                f.pad("box[")?;
                write!(f, "{ty}")?;
                f.pad("]")
            }
            Self::List(ty) => {
                f.pad("list[")?;
                write!(f, "{ty}")?;
                f.pad("]")
            }
            Self::Option(ty) => {
                f.pad("option[")?;
                write!(f, "{ty}")?;
                f.pad("]")
            }
            Self::Imm(region, ty) => {
                f.pad("imm [")?;
                region.fmt(f)?;
                f.pad("] ")?;
                write!(f, "{ty}")
            }
            Self::Mut(region, ty) => {
                f.pad("mut [")?;
                region.fmt(f)?;
                f.pad("] ")?;
                write!(f, "{ty}")
            }
            Self::Function(FunctionType {
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
                    param.fmt(f)?;
                    first = false;
                }
                f.pad(match *resource {
                    IsResource::Data => ") -> ",
                    IsResource::Resource => ") => ",
                })?;
                write!(f, "{return_type}")
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
            | Type::RawPointer
            | Type::String
            | Type::Infer(_)
            | Type::Param(..) => Ok(ty),
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
    fn apply_map<M: TypeMap>(self, m: &mut M) -> Result<Self, M::Error>
    where
        Self: Sized;
}

impl TypeMappable for Type {
    fn apply_map<M: TypeMap>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_type(self)
    }
}
impl TypeMappable for Region {
    fn apply_map<M: TypeMap>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_region(self)
    }
}

impl TypeMappable for FunctionType {
    fn apply_map<M: TypeMap>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_function_type(self)
    }
}
impl TypeMappable for RecordField {
    fn apply_map<M: TypeMap>(self, m: &mut M) -> Result<Self, M::Error> {
        m.map_field(self)
    }
}
