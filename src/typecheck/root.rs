use std::{
    cell::RefCell,
    collections::{HashMap, hash_map::Entry},
};

use crate::{
    ast::{self, Ident, Mutable, Param, Program},
    diagnostics::DiagnosticReporter,
    typecheck::{
        infer::TypeInfer,
        scheme::Scheme,
        types::{FunctionType, GenericArg, GenericKind, Region, Type},
    },
};
#[derive(Clone)]
struct GenericInfo {
    name: Ident,
    kind: GenericKind,
}
struct FunctionInfo {
    line: usize,
    generics: Vec<GenericInfo>,
    params: Vec<Param>,
    return_type: ast::Type,
}
#[derive(Debug)]
struct VarInfo {
    ty: Type,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Builtin {
    Alloc,
    DestroyBox,
    DestroyList,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Res {
    LocalRegion(usize),
    Param(usize),
    Builtin(Builtin),
    Function(usize),
    Var(usize),
}
struct GenericInfer {
    kinds: HashMap<String, GenericKind>,
}
fn infer_generic_kinds_region(region: &ast::Region, infer: &mut GenericInfer) {
    match region {
        ast::Region::Static(_) => (),
        ast::Region::Named(name) => {
            if let Entry::Vacant(vacant) = infer.kinds.entry(name.content.clone()) {
                vacant.insert(GenericKind::Region);
            }
        }
    }
}
fn infer_generic_kinds_ty(ty: &ast::Type, infer: &mut GenericInfer) {
    match ty {
        ast::Type::Bool | ast::Type::Int | ast::Type::String | ast::Type::Unit => (),
        ast::Type::Option(ty) | ast::Type::List(ty) | ast::Type::Ref(ty) => {
            infer_generic_kinds_ty(ty, infer);
        }
        ast::Type::Named(name) => {
            if let Entry::Vacant(vacant) = infer.kinds.entry(name.content.clone()) {
                vacant.insert(GenericKind::Type);
            }
        }
        ast::Type::Imm(region, ty) | ast::Type::Mut(region, ty) => {
            infer_generic_kinds_region(region, infer);
            infer_generic_kinds_ty(ty, infer);
        }
        ast::Type::Function(ast::FunctionType {
            params,
            return_type,
        }) => {
            for param in params {
                infer_generic_kinds_ty(param, infer);
            }
            infer_generic_kinds_ty(return_type, infer);
        }
    }
}
pub struct TypeCheck {
    functions: Vec<FunctionInfo>,
    pub(super) diag: RefCell<DiagnosticReporter>,
    variables: Vec<VarInfo>,
    generics: Vec<GenericKind>,
    regions: usize,
    env: HashMap<String, Res>,
    signatures: Vec<Scheme<FunctionType>>,
    pub(super) infer: TypeInfer,
}

impl TypeCheck {
    pub fn new(program: &Program) -> Self {
        let mut env = HashMap::from([
            (String::from("alloc"), Res::Builtin(Builtin::Alloc)),
            (
                String::from("destroy_box"),
                Res::Builtin(Builtin::DestroyBox),
            ),
            (
                String::from("destroy_list"),
                Res::Builtin(Builtin::DestroyList),
            ),
        ]);
        let functions = program
            .functions
            .iter()
            .enumerate()
            .map(|(i, function)| {
                env.insert(function.name.content.clone(), Res::Function(i));
                FunctionInfo {
                    line: function.name.line,
                    generics: match function.generics {
                        None => Vec::new(),
                        Some(ref generics) => {
                            let infer = &mut GenericInfer {
                                kinds: HashMap::new(),
                            };
                            for param in function.params.iter() {
                                infer_generic_kinds_ty(&param.ty, infer);
                            }
                            infer_generic_kinds_ty(&function.return_type, infer);
                            generics
                                .names
                                .iter()
                                .map(|name| match infer.kinds.get(&name.content) {
                                    Some(&kind) => GenericInfo {
                                        name: name.clone(),
                                        kind,
                                    },
                                    None => GenericInfo {
                                        name: name.clone(),
                                        kind: GenericKind::Type,
                                    },
                                })
                                .collect()
                        }
                    },
                    params: function.params.clone(),
                    return_type: function.return_type.clone(),
                }
            })
            .collect();
        Self {
            signatures: Vec::new(),
            regions: 0,
            infer: TypeInfer::new(),
            functions,
            env,
            generics: Vec::new(),
            diag: RefCell::new(DiagnosticReporter::new()),
            variables: Vec::new(),
        }
    }
    pub(super) fn get_res(&self, name: &str) -> Option<Res> {
        self.env.get(name).copied()
    }
    pub(super) fn lower_region(&self, region: &ast::Region) -> Region {
        match region {
            ast::Region::Named(name) => match self.get_res(&name.content) {
                None => {
                    self.diag
                        .borrow_mut()
                        .report(format!("'{}' not in scope", name.content), name.line);
                    Region::Unknown
                }
                Some(Res::LocalRegion(region)) => Region::Local(name.content.clone(), region),
                Some(Res::Param(region)) if let GenericKind::Region = self.generics[region] => {
                    Region::Param(name.content.clone(), region)
                }
                _ => {
                    self.diag.borrow_mut().report(
                        format!("Cannot use '{}' as region", name.content),
                        name.line,
                    );
                    Region::Unknown
                }
            },
            ast::Region::Static(_) => Region::Static,
        }
    }
    pub(super) fn lower_types(&self, tys: &mut dyn Iterator<Item = &ast::Type>) -> Vec<Type> {
        tys.map(|ty| self.lower_type(ty)).collect()
    }
    pub(super) fn lower_type(&self, ty: &ast::Type) -> Type {
        match ty {
            ast::Type::Bool => Type::Bool,
            ast::Type::Int => Type::Int,
            ast::Type::Unit => Type::Unit,
            ast::Type::String => Type::String,
            ast::Type::Option(ty) => Type::Option(Box::new(self.lower_type(ty))),
            ast::Type::Ref(ty) => Type::Ref(Box::new(self.lower_type(ty))),
            ast::Type::List(ty) => Type::List(Box::new(self.lower_type(ty))),
            ast::Type::Imm(region, ty) => {
                let region = self.lower_region(region);
                let ty = self.lower_type(ty);
                Type::Imm(region, Box::new(ty))
            }
            ast::Type::Mut(region, ty) => {
                let region = self.lower_region(region);
                let ty = self.lower_type(ty);
                Type::Mut(region, Box::new(ty))
            }
            ast::Type::Function(function) => {
                let params = self.lower_types(&mut function.params.iter());
                let return_type = self.lower_type(&function.return_type);
                Type::Function(FunctionType {
                    params,
                    return_type: Box::new(return_type),
                })
            }
            ast::Type::Named(name) => match self.get_res(&name.content) {
                Some(res) => match res {
                    Res::Param(param) if let GenericKind::Type = self.generics[param] => {
                        Type::Param(name.content.clone(), param)
                    }
                    _ => {
                        self.diag.borrow_mut().report(
                            format!("Cannot use '{}' as a type", name.content),
                            name.line,
                        );
                        Type::Unknown
                    }
                },
                None => {
                    self.diag
                        .borrow_mut()
                        .report(format!("'{}' not in scope", name.content), name.line);
                    Type::Unknown
                }
            },
        }
    }
    pub(super) fn iterator_element(&self, ty: Type) -> Option<Type> {
        match ty {
            Type::Imm(_, ty) | Type::Mut(_, ty) => match self.simplify(*ty) {
                Type::List(element) => Some(*element),
                Type::String => todo!("Charssss"),
                ty => self.iterator_element(ty),
            },
            Type::Infer(var) => match self.simplify(Type::Infer(var)) {
                Type::Infer(_) => None,
                ty => self.iterator_element(ty),
            },
            Type::Unknown => Some(Type::Unknown),
            Type::Bool
            | Type::Int
            | Type::Param(..)
            | Type::Unit
            | Type::List(_)
            | Type::String
            | Type::Option(_)
            | Type::Function(_)
            | Type::Ref(_) => None,
        }
    }
    pub(super) fn signature_of_builtin(&self, builtin: Builtin) -> Scheme<FunctionType> {
        match builtin {
            Builtin::Alloc => Scheme::new(
                FunctionType {
                    params: vec![Type::Param("T".to_string(), 0)],
                    return_type: Box::new(Type::Ref(Box::new(Type::Param("T".to_string(), 0)))),
                },
                1,
            ),
            Builtin::DestroyBox => Scheme::new(
                FunctionType {
                    params: vec![
                        Type::Ref(Box::new(Type::Param("T".to_string(), 0))),
                        Type::Function(FunctionType {
                            params: vec![Type::Param("T".to_string(), 0)],
                            return_type: Box::new(Type::Unit),
                        }),
                    ],
                    return_type: Box::new(Type::Unit),
                },
                1,
            ),
            Builtin::DestroyList => Scheme::new(
                FunctionType {
                    params: vec![
                        Type::List(Box::new(Type::Param("T".to_string(), 0))),
                        Type::Function(FunctionType {
                            params: vec![Type::Param("T".to_string(), 0)],
                            return_type: Box::new(Type::Unit),
                        }),
                    ],
                    return_type: Box::new(Type::Unit),
                },
                1,
            ),
        }
    }
    pub(super) fn signature_of_function(&self, function: usize) -> Scheme<FunctionType> {
        self.signatures[function].clone()
    }
    pub(super) fn find_signature_of(&self, function: &str) -> Option<Scheme<FunctionType>> {
        self.env.get(function).and_then(|res| match res {
            Res::Builtin(builtin) => Some(self.signature_of_builtin(*builtin)),
            Res::Function(function) => Some(self.signature_of_function(*function)),
            Res::Var(_) | Res::Param(_) | Res::LocalRegion(_) => None,
        })
    }
    pub(super) fn instantiate_builtin_args(
        &mut self,
        builtin: Builtin,
        line: usize,
    ) -> Vec<GenericArg> {
        match builtin {
            Builtin::Alloc | Builtin::DestroyBox | Builtin::DestroyList => {
                vec![GenericArg::Type(Type::Infer(self.infer.fresh_ty(line)))]
            }
        }
    }
    pub(super) fn fresh_ty(&mut self, line: usize) -> Type {
        Type::Infer(self.infer.fresh_ty(line))
    }
    pub(super) fn instantiate_function_args(
        &mut self,
        function: usize,
        line: usize,
    ) -> Vec<GenericArg> {
        self.functions[function]
            .generics
            .iter()
            .map(|arg| match arg.kind {
                GenericKind::Region => {
                    GenericArg::Region(Region::Infer(self.infer.fresh_region(line)))
                }
                GenericKind::Type => GenericArg::Type(Type::Infer(self.infer.fresh_ty(line))),
            })
            .collect()
    }
    pub(super) fn var_type(&self, var: usize) -> &Type {
        &self.variables[var].ty
    }
    pub(super) fn declare_var(&mut self, _mutable: Mutable, var_name: &str, ty: Type) {
        let next_var = self.variables.len();
        self.variables.push(VarInfo { ty });
        self.env.insert(var_name.to_string(), Res::Var(next_var));
    }
    pub(super) fn declare_region(&mut self, name: &str) -> usize {
        let next_region = self.regions;
        self.regions += 1;
        self.env
            .insert(name.to_string(), Res::LocalRegion(next_region));
        next_region
    }
    pub(super) fn simplify(&self, ty: Type) -> Type {
        self.infer.simplify(ty)
    }
    pub(super) fn declare_generic(&mut self, param: &str, kind: GenericKind) {
        let next_generic = self.generics.len();
        self.generics.push(kind);
        self.env.insert(param.to_string(), Res::Param(next_generic));
    }
    pub(super) fn unify(&mut self, ty1: Type, ty2: Type, line: usize) -> Type {
        if let Some(ty) = self.infer.unify_ty(ty1.clone(), ty2.clone()) {
            ty
        } else {
            let ty1 = self.simplify(ty1);
            let ty2 = self.simplify(ty2);
            self.diag
                .borrow_mut()
                .report(format!("Expected '{ty1}' but got '{ty2}'"), line);
            Type::Unknown
        }
    }

    pub(super) fn type_annotations_needed(&self, line: usize) {
        self.diag
            .borrow_mut()
            .report("type annotations needed".to_string(), line);
    }
    pub(super) fn in_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let old_env = self.env.clone();
        let value = f(self);
        self.env = old_env;
        value
    }
    fn validate_main(&mut self, program: &Program) {
        let Some(main) = self.env.get("main").and_then(|res| match res {
            &Res::Function(function) => Some(function),
            _ => None,
        }) else {
            return self.diag.borrow_mut().report(
                "Missing main".to_string(),
                program
                    .functions
                    .last()
                    .map(|function| function.body.line)
                    .unwrap_or(1),
            );
        };

        let main = &self.functions[main];
        if !main.generics.is_empty() {
            self.diag
                .borrow_mut()
                .report("'main' should not be generic".to_string(), main.line);
        }
        if !main.params.is_empty() {
            self.diag
                .borrow_mut()
                .report("'main' should have no parameters".to_string(), main.line);
        }
        if !matches!(main.return_type, ast::Type::Unit) {
            self.diag.borrow_mut().report(
                "'main' should have '()' as return type".to_string(),
                main.line,
            );
        }
    }
    pub fn check(mut self, program: &Program) -> bool {
        self.validate_main(program);
        for (f, _) in program.functions.iter().enumerate() {
            let () = self.in_scope(|this| {
                for param_info in this.functions[f].generics.clone() {
                    this.declare_generic(&param_info.name.content, param_info.kind);
                }
                let function = &this.functions[f];
                let param_count = function.generics.len();
                let signature = Scheme::new(
                    FunctionType {
                        params: this
                            .lower_types(&mut function.params.iter().map(|param| &param.ty)),
                        return_type: Box::new(this.lower_type(&function.return_type)),
                    },
                    param_count,
                );
                this.signatures.push(signature);
                this.generics.clear();
                this.infer.clear();
            });
        }
        for (function_index, function) in program.functions.iter().enumerate() {
            let () = self.in_scope(|this| {
                for param_info in this.functions[function_index].generics.clone() {
                    this.declare_generic(&param_info.name.content, param_info.kind);
                }
                let FunctionType {
                    params,
                    return_type,
                } = this
                    .find_signature_of(&function.name.content)
                    .expect("All functions should be defined")
                    .skip();
                for (param_name, ty) in function.params.iter().map(|param| &param.name).zip(params)
                {
                    this.declare_var(Mutable::Immutable, &param_name.content, ty);
                }
                this.check_expr(&function.body, Some(*return_type));
                for line in this.infer.unsolved_var_lines() {
                    this.diag
                        .borrow_mut()
                        .report("type annotations needed".to_string(), line);
                }
                this.variables.clear();
                this.generics.clear();
                this.infer.clear();
            });
        }
        !self.diag.into_inner().finish()
    }
}
