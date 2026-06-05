use std::collections::HashMap;

use crate::{
    ident::Ident,
    interpret::values::Pointer,
    resolved_ast::{FunctionId, VarId},
    typed_ast::{self, Function},
    typed_ast_visitor::{Visitor, walk_expr, walk_pattern},
    types::Type,
};
pub struct FunctionInstance {
    pub id: FunctionId,
    pub args: Vec<Type>,
}
#[derive(Clone, Copy)]
pub struct FunctionInfo<'f> {
    pub code: &'f Function,
    pub pointer: Pointer,
}
