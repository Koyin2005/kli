use std::{
    fmt::{Debug, Display},
    ops::ControlFlow,
};

use crate::{
    Symbol,
    ast::{IsResource, Mutable},
    collect::{CtxtRef, TypeDefKind},
    def_ids::DefId,
    define_id,
    index_vec::IndexVec,
    lang_items::LangItem,
    resolved_ast::LocalRegionId,
    typed_ast::{Capture, FieldId},
};
define_id!(CaseId);
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
    pub name: Symbol,
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
impl Region {
    pub const fn no_op_visit<T>(self) -> ControlFlow<T> {
        ControlFlow::Continue(())
    }
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
    Char,
    Byte,
    Never,
    Param(Symbol, usize),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
    Function(FunctionType),
    Tuple(Box<[Type]>),
    Record(IndexVec<FieldId, RecordField>),
    RawPointer(Box<Type>),
    Array(Box<Type>, u64),
    Named(DefId, Symbol, GenericArgs),
}
impl Type {
    pub fn string(ctxt: CtxtRef<'_>) -> Self {
        let id = ctxt.lang_items().expect(LangItem::String);
        let name = ctxt.expect_ident(id).symbol;
        Type::Named(id, name, GenericArgs::new())
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
    pub fn is_reference(&self) -> bool {
        matches!(self, Self::Imm(..) | Self::Mut(..))
    }
    pub fn reference(self, mutable: Mutable, region: Region) -> Self {
        match mutable {
            Mutable::Immutable => Self::Imm(region, Box::new(self)),
            Mutable::Mutable => Self::Mut(region, Box::new(self)),
        }
    }
    pub fn into_pointer_type(self, ctxt: CtxtRef<'_>) -> Result<(PointerType, Self), Self> {
        match self {
            Self::RawPointer(ty) => Ok((PointerType::Raw, *ty)),
            Self::Named(..) => Ok((PointerType::Box, self.into_box(ctxt)?)),
            Self::Imm(region, ty) => Ok((PointerType::Reference(region, Mutable::Immutable), *ty)),
            Self::Mut(region, ty) => Ok((PointerType::Reference(region, Mutable::Mutable), *ty)),
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
        let GenericArg::Type(ty) = arg else {
            unreachable!()
        };
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
        let GenericArg::Type(ty) = arg else {
            return None;
        };
        Some(ty)
    }
    pub fn pointer_kind(&self, ctxt: CtxtRef<'_>) -> Option<PointerType> {
        match self {
            Self::RawPointer(_) => Some(PointerType::Raw),
            Self::Named(..) if self.as_box(ctxt).is_some() => Some(PointerType::Box),
            &Self::Imm(region, _) => Some(PointerType::Reference(region, Mutable::Immutable)),
            &Self::Mut(region, _) => Some(PointerType::Reference(region, Mutable::Mutable)),
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
            })
            | Type::Never => false,
            Type::Array(ty, _) => ty.is_resource(ctxt),
            Type::Mut(..)
            | Type::Function(FunctionType {
                resource: IsResource::Resource,
                ..
            })
            | Type::Param(..) => true,
            Type::Record(fields) => fields.iter().any(|field| field.ty.is_resource(ctxt)),
            Type::Tuple(fields) => fields.iter().any(|field| field.is_resource(ctxt)),
            Type::Infer(_) => unreachable!("Cannot 'infer' its a resource"),
            &Type::Named(id, _, ref args) => {
                let is_copy = ctxt
                    .annotations(id)
                    .iter()
                    .any(|annotation| annotation.kind == crate::resolved_ast::AnnotationKind::Copy);
                if !is_copy || ctxt.is_type_recursive(id) {
                    return true;
                }
                ctxt.type_def(id)
                    .all_fields()
                    .any(|field| field.type_of(args, ctxt).is_resource(ctxt))
            }
        }
    }
    pub const fn no_op_visit<T>(&self) -> ControlFlow<T> {
        ControlFlow::Continue(())
    }
    pub fn is_uninhabited(&self, ctxt: CtxtRef<'_>) -> bool {
        match self {
            Type::Infer(_)
            | Type::Unknown
            | Type::Unit
            | Type::Int
            | Type::Bool
            | Type::Char
            | Type::Byte
            | Type::Param(..)
            | Type::Function(..) => false,
            Type::Never => true,
            Type::Imm(_, ty) | Type::Mut(_, ty) => ty.is_uninhabited(ctxt),
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
            | Type::Char
            | Type::Byte
            | Type::Param(..)
            | Type::Never => ControlFlow::Continue(()),
            Type::RawPointer(ty) | Type::Array(ty, _) => ty.visit(visit_ty, visit_region),
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
            Type::Tuple(fields) => {
                for field in fields {
                    visit_ty(field)?;
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
                    first = false;
                }
                f.pad(")")
            }
            Type::Char => f.pad("char"),
            Type::Bool => f.pad("bool"),
            Type::Int => f.pad("int"),
            Type::Unit => f.pad("()"),
            Type::Unknown => f.pad("{unknown}"),
            Type::Infer(_) => f.pad("_"),
            &Type::Param(name, _) => write!(f, "{}", name),
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
            | Type::Byte
            | Type::Infer(_)
            | Type::Param(..)
            | Type::Never => Ok(ty),
            Type::Array(ty, count) => Ok(Type::Array(Box::new(self.map_type(*ty)?), count)),
            Type::RawPointer(ty) => Ok(Type::RawPointer(Box::new(self.map_type(*ty)?))),
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
