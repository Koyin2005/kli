use std::collections::HashMap;

use crate::{ast::BinaryOp, interpret::{functions::{ FunctionInfo, FunctionInstance}, values::Value}, resolved_ast::FunctionId, src_loc::SrcLoc, typed_ast, types::Type};

mod values;
mod functions;
#[derive(Debug)]
pub enum InterpretErrorKind {
    Panic,
    Overflow,
    DivideByZero
}
#[derive(Debug)]
pub struct InterpretError {
    pub loc : SrcLoc,
    pub kind : InterpretErrorKind
}

pub struct Interpret<'f>{
    functions : HashMap<FunctionId,FunctionInfo<'f>>,
    entry : FunctionId,
    call_stack : Vec<FunctionId>
}
impl<'f> Interpret<'f>{
    pub fn new(functions : &'f [typed_ast::Function]) -> Self{
        let entry = functions.iter().position(|f|{
            &f.name.content as &str == "main"
        }).map(FunctionId::new).expect("Should have an entry point");
        let functions = functions.iter().enumerate().map(|(i,function)|{
            (FunctionId::new(i), FunctionInfo{
                code:function
            })
        }).collect();
        Self { functions, entry, call_stack: vec![] }
    }
    fn interpet_stmt(&mut self){

    }
    fn interpret_expr(&mut self, expr: &typed_ast::Expr) -> Result<Value,InterpretError>{
        match &expr.kind{
            typed_ast::ExprKind::Err => panic!("Cannot interpret err value"),
            typed_ast::ExprKind::Panic => Err(InterpretError { loc: expr.loc.clone(), kind: InterpretErrorKind::Panic }),
            typed_ast::ExprKind::Int(value) => {
                Ok(Value::Int(*value as i128))
            },
            typed_ast::ExprKind::Binary(op,left,right) => {
                let left = self.interpret_expr(left)?.as_int().unwrap();
                let right = self.interpret_expr(right)?.as_int().unwrap();
                let (res,overflow) = match op{
                    BinaryOp::Add => left.overflowing_add(right),
                    BinaryOp::Divide => if right == 0{
                        return Err(InterpretError { loc: expr.loc.clone(), kind: InterpretErrorKind::DivideByZero })
                    }else { left.overflowing_div(right)},
                    BinaryOp::Subtract => left.overflowing_sub(right),
                    BinaryOp::Multiply => left.overflowing_mul(right),
                };
                if overflow{
                     Err(InterpretError { loc: expr.loc.clone(), kind: InterpretErrorKind::Overflow })
                }
                else {
                    Ok(Value::Int(res))
                }
            },
            typed_ast::ExprKind::Bool(value) => Ok(Value::Bool(*value)),
            _ => todo!()
        }
    }
    fn interpret_function(&mut self, f : FunctionId) -> Result<(),InterpretError>{
        self.call_stack.push(f);
        
        self.call_stack.pop();
        Ok(())
    }
    pub fn interpret(mut self) -> Result<(),InterpretError>{
        self.interpret_function(self.entry)
    }

}