use crate::{ident::Ident, resolved_ast::FunctionId, types::Type};
pub struct FunctionInstance{
    pub id : FunctionId,
    pub args : Vec<Type>
}

pub struct Function{
    pub name : Ident,
}