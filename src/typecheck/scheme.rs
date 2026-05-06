use crate::types::{FunctionType, GenericArg, Region, Type};
#[derive(Clone)]
pub struct Scheme<T> {
    value: T,
    _param_count: usize,
}
impl<T: Bind> Scheme<T> {
    pub fn new(value: T, param_count: usize) -> Self {
        Self {
            value,
            _param_count: param_count,
        }
    }
    pub fn bind(self, args: &[GenericArg]) -> T {
        self.value.bind(args)
    }
    pub fn skip(self) -> T {
        self.value
    }
}
pub trait Bind {
    fn bind(self, args: &[GenericArg]) -> Self;
}
impl Bind for Region {
    fn bind(self, args: &[GenericArg]) -> Self {
        match self {
            Self::Static | Self::Unknown | Self::Infer(_) | Self::Local(..) => self,
            Self::Param(_, index) => {
                if let Some(GenericArg::Region(region)) = args.get(index) {
                    region.clone()
                } else {
                    Region::Unknown
                }
            }
        }
    }
}
impl Bind for Type {
    fn bind(self, args: &[GenericArg]) -> Self {
        match self {
            Self::Bool
            | Self::Int
            | Self::Unit
            | Self::Unknown
            | Self::String
            | Self::Infer(_)
            | Self::Char => self,
            Self::Imm(region, ty) => Self::Imm(region.bind(args), Box::new((*ty).bind(args))),
            Self::Mut(region, ty) => Self::Mut(region.bind(args), Box::new((*ty).bind(args))),
            Self::Function(function) => Self::Function(function.bind(args)),
            Self::Box(ty) => Self::Box(Box::new((*ty).bind(args))),
            Self::List(ty) => Self::List(Box::new((*ty).bind(args))),
            Self::Option(ty) => Self::Option(Box::new((*ty).bind(args))),
            Self::Param(_, index) => {
                if let Some(GenericArg::Type(ty)) = args.get(index) {
                    ty.clone()
                } else {
                    Self::Unknown
                }
            }
        }
    }
}
impl Bind for FunctionType {
    fn bind(self, args: &[GenericArg]) -> Self {
        Self {
            resource: self.resource,
            params: self.params.into_iter().map(|ty| ty.bind(args)).collect(),
            return_type: Box::new(self.return_type.bind(args)),
        }
    }
}
