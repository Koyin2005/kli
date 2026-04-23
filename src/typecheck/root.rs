use std::collections::HashMap;

use crate::{ast::{self, Function, FunctionType, Generics, Param, Program}, diagnostics::DiagnosticReporter, typecheck::types::Type};

struct FunctionInfo{
    generics : Option<Generics>,
    params : Vec<Param>,
    return_type : ast::Type
}
struct VarInfo{
    ty : Type
}
enum Builtin {
    Alloc,
    Free
}
enum Res {
    Builtin(Builtin),
    Function(usize),
    Var(usize),
}
pub struct TypeCheck{
    functions : Vec<FunctionInfo>,
    pub(super) diag : DiagnosticReporter,
    variables : Vec<VarInfo>,
    env : HashMap<String,Res>
}

impl TypeCheck{
    pub fn new(program : &Program) -> Self{
        let mut env = HashMap::from([
            (String::from("alloc"),Res::Builtin(Builtin::Alloc)),
            (String::from("free"),Res::Builtin(Builtin::Free)),
        ]);
        let functions = program.functions.iter().enumerate().map(|(i,function)|{
            env.insert(function.name.content.clone(),Res::Function(i));
            FunctionInfo{
                generics : function.generics.clone(),
                params: function.params.clone(),
                 return_type: function.return_type.clone() 
                }
            
        }).collect();
        Self { functions,env, diag: DiagnosticReporter::new(),variables:Vec::new() }
    }
    pub(super) fn signature_of(&self, function: &str) -> Option<(Vec<Type>,Type)>{
        None
    }
    pub(super) fn declare_var(&mut self, var_name: &str, ty: Type){
        let next_var = self.variables.len();
        self.variables.push(VarInfo { ty });
        self.env.insert(var_name.to_string(), Res::Var(next_var));
    }
    pub(super) fn unify(&mut self, ty1: Type, ty2: Type, line: usize) -> Type{
        if ty1 == ty2{
            ty1
        }
        else {
            self.diag.report(format!("Expected '{ty1}' but got '{ty2}'"),line);
            Type::Unknown
        }
    }
    pub fn check(mut self, program : &Program) {
        for function in &program.functions{
            let (params,return_ty) = self.signature_of(&function.name.content).expect("All functions should be defined");
            for (param_name,ty) in function.params.iter().map(|param| &param.name).zip(params){
                self.declare_var(&param_name.content, ty);
            }
            self.check_expr(&function.body, Some(return_ty));
            self.variables.clear();
        }
    }
}