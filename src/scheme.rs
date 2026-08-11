use crate::{
    CtxtRef,
    types::{GenericArg, GenericArgsRef, Type, TypeKind, TypeMap, TypeMappable},
};
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Scheme<T> {
    value: T,
}
impl<'ctxt, T: TypeMappable<'ctxt>> Scheme<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Scheme<U> {
        Scheme {
            value: f(self.value),
        }
    }
    pub fn bind(self, ctxt: CtxtRef<'ctxt>, args: GenericArgsRef<'_, 'ctxt>) -> T {
        struct Binder<'ctxt, 'b>(CtxtRef<'ctxt>, GenericArgsRef<'b, 'ctxt>);
        impl<'ctxt> TypeMap<'ctxt> for Binder<'ctxt, '_> {
            type Error = std::convert::Infallible;
            fn ctxt(&self) -> crate::CtxtRef<'ctxt> {
                self.0
            }
            fn map_type(&mut self, ty: Type<'ctxt>) -> Result<Type<'ctxt>, Self::Error> {
                let &TypeKind::Param(_, index) = ty.kind() else {
                    return self.super_map_type(ty);
                };
                let Some(GenericArg(ty)) = self.1.get(index).cloned() else {
                    return Ok(Type::UNKNOWN);
                };
                Ok(ty)
            }
        }
        let Ok(value) = self.value.apply_map(&mut Binder(ctxt, args));
        value
    }
    pub fn skip(self) -> T {
        self.value
    }
}
