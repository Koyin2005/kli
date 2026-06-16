use crate::types::{GenericArg, Region, Type, TypeMap, TypeMappable};
#[derive(Clone, Eq, PartialEq)]
pub struct Scheme<T> {
    value: T,
}
impl<T: TypeMappable> Scheme<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
    pub fn bind(self, args: &[GenericArg]) -> T {
        struct Binder<'a>(&'a [GenericArg]);
        impl TypeMap for Binder<'_> {
            type Error = std::convert::Infallible;
            fn map_type(&mut self, ty: Type) -> Result<Type, Self::Error> {
                let Type::Param(_, index) = ty else {
                    return self.super_map_type(ty);
                };
                let Some(GenericArg::Type(ty)) = self.0.get(index).cloned() else {
                    return Ok(Type::Unknown);
                };
                Ok(ty)
            }
            fn map_region(&mut self, region: Region) -> Result<Region, Self::Error> {
                let Region::Param(_, index) = region else {
                    return self.super_map_region(region);
                };
                let Some(GenericArg::Region(region)) = self.0.get(index).cloned() else {
                    return Ok(Region::Unknown);
                };
                Ok(region)
            }
        }
        let Ok(value) = self.value.apply_map(&mut Binder(args));
        value
    }
    pub fn skip(self) -> T {
        self.value
    }
}
