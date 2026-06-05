use crate::{
    interpret::values::Pointer,
    typed_ast::{Expr, Param},
};
#[derive(Clone, Copy)]
pub struct FunctionInfo<'f> {
    pub params: &'f [Param],
    pub body: &'f Expr,
    pub pointer: Pointer,
}
