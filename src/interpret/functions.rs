use std::collections::HashMap;

use crate::{ident::Ident, resolved_ast::FunctionId, typed_ast::Function, types::Type};
pub struct FunctionInstance{
    pub id : FunctionId,
    pub args : Vec<Type>
}

pub struct FunctionInfo<'f>{
    pub code : &'f Function,
}