use crate::{interpret::values::Pointer, typed_ast::Function};
#[derive(Clone, Copy)]
pub struct FunctionInfo<'f> {
    pub code: &'f Function,
    pub pointer: Pointer,
}
