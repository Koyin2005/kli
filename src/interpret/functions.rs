use crate::{interpret::values::Value, resolved_ast::FunctionId, typed_ast::{Expr, GenericParam, Param}, types::{GenericArg, Type}};
#[derive(Clone, Copy)]
pub struct FunctionInfo<'f> {
    pub generics: &'f [GenericParam],
    pub params: &'f [Param],
    pub body: Option<&'f Expr>,
}