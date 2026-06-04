use std::collections::HashMap;

use crate::{ast::Function, interpret::functions::FunctionInstance};

mod values;
mod functions;
pub struct Interpret{
    functions : HashMap<FunctionInstance,Function>
}
impl Interpret{
    pub fn new(entry : FunctionInstance) -> Self{
        Self { functions: HashMap::new() }
    }

    pub fn interpret(self){}

}