use crate::types::{GenericArg, GenericArgsRef, Type, TypeMap, TypeMappable};
#[derive(Clone, Eq, PartialEq)]
pub struct Scheme<T> {
    value: T,
}
impl<T: TypeMappable> Scheme<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Scheme<U> {
        Scheme {
            value: f(self.value),
        }
    }
    pub fn bind(self, args: GenericArgsRef<'_>) -> T {
        struct Binder<'a>(GenericArgsRef<'a>);
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
        }
        let Ok(value) = self.value.apply_map(&mut Binder(args));
        value
    }
    pub fn skip(self) -> T {
        self.value
    }
}
