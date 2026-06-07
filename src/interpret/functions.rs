use crate::typed_ast::{Expr, GenericParam, Param};
#[derive(Clone, Copy)]
pub struct FunctionInfo<'f> {
    pub generics: &'f [GenericParam],
    pub params: &'f [Param],
    pub body: &'f Expr,
}
