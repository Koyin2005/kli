use std::fmt::Display;

use crate::ast::Mutable;
#[derive(Clone, Copy, Debug)]
pub enum GenericKind {
    Region,
    Type,
}
#[derive(Debug)]
pub enum GenericArg {
    Region(Region),
    Type(Type),
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub return_type: Box<Type>,
}
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Region {
    Unknown,
    Static,
    Param(String, usize),
    Local(String, usize),
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
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Type {
    Infer(usize),
    Unknown,
    Unit,
    Int,
    Bool,
    String,
    Param(String, usize),
    Ref(Box<Type>),
    List(Box<Type>),
    Option(Box<Type>),
    Imm(Region, Box<Type>),
    Mut(Region, Box<Type>),
    Function(FunctionType),
}
impl Type {
    pub fn as_reference_type(self) -> Result<(Mutable, Region, Self), Self> {
        let (region, mutable, ty) = match self {
            Self::Imm(region, ty) => (region, Mutable::Immutable, *ty),
            Self::Mut(region, ty) => (region, Mutable::Mutable, *ty),
            _ => return Err(self),
        };
        Ok((mutable, region, ty))
    }
}
impl Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool => f.pad("bool"),
            Self::Int => f.pad("int"),
            Self::Unit => f.pad("unit"),
            Self::Unknown => f.pad("{unknown}"),
            Self::String => f.pad("string"),
            Self::Infer(_) => f.pad("_"),
            Self::Param(name, _) => f.pad(name),
            Self::Ref(ty) => {
                f.pad("ref[")?;
                ty.fmt(f)?;
                f.pad("]")
            }
            Self::List(ty) => {
                f.pad("list[")?;
                ty.fmt(f)?;
                f.pad("]")
            }
            Self::Option(ty) => {
                f.pad("option[")?;
                ty.fmt(f)?;
                f.pad("]")
            }
            Self::Imm(region, ty) => {
                f.pad("imm [")?;
                region.fmt(f)?;
                f.pad("] ")?;
                ty.fmt(f)
            }
            Self::Mut(region, ty) => {
                f.pad("mut [")?;
                region.fmt(f)?;
                f.pad("] ")?;
                ty.fmt(f)
            }
            Self::Function(FunctionType {
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
                f.pad(") -> ")?;
                return_type.fmt(f)
            }
        }
    }
}
