use std::{cell::RefCell, collections::HashMap};

use crate::{
    ast::Ident,
    diagnostics::DiagnosticReporter,
    resolved_ast::{self as res, Builtin, FunctionId, Program, VarId},
    scheme::Scheme,
    typecheck::{infer::TypeInfer, lower::Lower, subst::TypeSubst},
    typed_ast::{self, Function, GenericParam},
    types::{FunctionType, GenericArg, GenericKind, Region, Type},
};
pub struct TypeError;
#[derive(Debug)]
struct VarInfo {
    ty: Type,
}
pub struct TypeCheck {
    function_generic_kinds: Vec<Vec<GenericKind>>,
    pub(super) diag: RefCell<DiagnosticReporter>,
    variables: Vec<VarInfo>,
    generics: Vec<GenericKind>,
    signatures: Vec<Scheme<FunctionType>>,
    pub(super) infer: TypeInfer,
}

impl TypeCheck {
    pub fn new(program: &Program) -> Self {
        let mut signatures = Vec::new();
        let diag = RefCell::new(DiagnosticReporter::new());
        let mut function_kinds = Vec::new();
        for function in program.functions.iter() {
            let kinds = match function.generics {
                None => Vec::new(),

                Some(ref generics) => generics
                    .kinds
                    .iter()
                    .map(|kind| match kind {
                        res::GenericKind::Region => GenericKind::Region,
                        res::GenericKind::Type => GenericKind::Type,
                    })
                    .collect::<Vec<_>>(),
            };
            let lower = Lower::new(&kinds, &diag);
            let signature = Scheme::new(FunctionType::new_data(
                lower.lower_types(&mut function.params.iter().map(|param| &param.ty)),
                lower.lower_type(&function.return_type),
            ));
            signatures.push(signature);
            function_kinds.push(kinds);
        }
        Self {
            generics: Vec::new(),
            signatures,
            infer: TypeInfer::new(),
            function_generic_kinds: function_kinds,
            diag: RefCell::new(DiagnosticReporter::new()),
            variables: Vec::new(),
        }
    }
    pub(super) fn iterator_element(&self, ty: Type) -> Result<Type, Type> {
        match ty {
            Type::Imm(_, _) | Type::Mut(_, _) => {
                let (mutable, region, ty) = ty.as_reference_type().expect("Should be a reference");
                let ty = self.simplify_type(ty.clone());
                match ty {
                    Type::List(element) => Ok(Type::reference(*element, mutable, region.clone())),
                    Type::String => Ok(Type::Char),
                    ty => self.iterator_element(ty),
                }
            }
            Type::Infer(var) => match self.simplify_type(Type::Infer(var)) {
                Type::Infer(_) => Err(ty),
                ty => self.iterator_element(ty),
            },
            Type::Unknown => Ok(Type::Unknown),
            Type::Bool
            | Type::Int
            | Type::Char
            | Type::Param(..)
            | Type::Unit
            | Type::List(_)
            | Type::String
            | Type::Option(_)
            | Type::Function(_)
            | Type::Box(_) => Err(ty),
        }
    }
    pub(super) fn signature_of_builtin(&self, builtin: Builtin) -> Scheme<FunctionType> {
        match builtin {
            Builtin::Replace => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![
                    Type::Mut(
                        Region::Param("r".to_string(), 0),
                        Box::new(Type::Param("T".to_string(), 1)),
                    ),
                    Type::Function(FunctionType {
                        resource: crate::ast::IsResource::Resource,
                        params: vec![Type::Param("T".to_string(), 1)],
                        return_type: (Box::new(Type::Param("T".to_string(), 1))),
                    }),
                ],
                return_type: Box::new(Type::Mut(
                    Region::Param("r".to_string(), 0),
                    Box::new(Type::Param("T".to_string(), 1)),
                )),
            }),
            Builtin::Swap => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![
                    Type::Mut(
                        Region::Param("r".to_string(), 0),
                        Box::new(Type::Param("T".to_string(), 1)),
                    ),
                    Type::Param("T".to_string(), 1),
                ],
                return_type: Box::new(Type::Param("T".to_string(), 1)),
            }),
            Builtin::DestroyString => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![Type::String],
                return_type: Box::new(Type::Unit),
            }),
            Builtin::AllocBox => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![Type::Param("T".to_string(), 0)],
                return_type: Box::new(Type::Box(Box::new(Type::Param("T".to_string(), 0)))),
            }),
            Builtin::DeallocBox => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![Type::Box(Box::new(Type::Param("T".to_string(), 0)))],
                return_type: Box::new(Type::Param("T".to_string(), 0)),
            }),
            Builtin::DerefBox => {
                let r_param = Region::Param("r".to_string(), 0);
                let t_param = Type::Param("T".to_string(), 1);
                Scheme::new(FunctionType {
                    resource: crate::ast::IsResource::Data,
                    params: vec![Type::Imm(
                        r_param.clone(),
                        Box::new(Type::Box(Box::new(t_param.clone()))),
                    )],
                    return_type: Box::new(Type::Imm(r_param, Box::new(t_param))),
                })
            }
            Builtin::DerefBoxMut => {
                let r_param = Region::Param("r".to_string(), 0);
                let t_param = Type::Param("T".to_string(), 1);
                Scheme::new(FunctionType {
                    resource: crate::ast::IsResource::Data,
                    params: vec![Type::Mut(
                        r_param.clone(),
                        Box::new(Type::Box(Box::new(t_param.clone()))),
                    )],
                    return_type: Box::new(Type::Mut(r_param, Box::new(t_param))),
                })
            }
            Builtin::DestroyList => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![
                    Type::List(Box::new(Type::Param("T".to_string(), 0))),
                    Type::Function(FunctionType {
                        resource: crate::ast::IsResource::Data,
                        params: vec![Type::Param("T".to_string(), 0)],
                        return_type: Box::new(Type::Unit),
                    }),
                ],
                return_type: Box::new(Type::Unit),
            }),
            Builtin::Freeze => Scheme::new(FunctionType {
                resource: crate::ast::IsResource::Data,
                params: vec![Type::Mut(
                    Region::Param("r".to_string(), 0),
                    Box::new(Type::Param("T".to_string(), 1)),
                )],
                return_type: Box::new(Type::Imm(
                    Region::Param("r".to_string(), 0),
                    Box::new(Type::Param("T".to_string(), 1)),
                )),
            }),
        }
    }
    pub(super) fn signature_of_function(&self, function: FunctionId) -> Scheme<FunctionType> {
        self.signatures[usize::from(function)].clone()
    }
    pub(super) fn instantiate_builtin_args(
        &mut self,
        builtin: Builtin,
        line: usize,
    ) -> Vec<GenericArg> {
        match builtin {
            Builtin::AllocBox | Builtin::DeallocBox | Builtin::DestroyList => {
                vec![GenericArg::Type(self.fresh_ty(line))]
            }
            Builtin::DerefBox
            | Builtin::DerefBoxMut
            | Builtin::Freeze
            | Builtin::Replace
            | Builtin::Swap => {
                vec![
                    GenericArg::Region(self.fresh_region(line)),
                    GenericArg::Type(self.fresh_ty(line)),
                ]
            }
            Builtin::DestroyString => Vec::new(),
        }
    }
    pub(super) fn fresh_region(&mut self, line: usize) -> Region {
        Region::Infer(self.infer.fresh_region(line))
    }
    pub(super) fn fresh_ty(&mut self, line: usize) -> Type {
        Type::Infer(self.infer.fresh_ty(line))
    }
    pub(super) fn instantiate_function_args(
        &mut self,
        function: FunctionId,
        line: usize,
    ) -> Vec<GenericArg> {
        self.function_generic_kinds[usize::from(function)]
            .iter()
            .map(|kind| match *kind {
                GenericKind::Region => {
                    GenericArg::Region(Region::Infer(self.infer.fresh_region(line)))
                }
                GenericKind::Type => GenericArg::Type(Type::Infer(self.infer.fresh_ty(line))),
            })
            .collect()
    }
    pub(super) fn var_type(&self, var: VarId) -> &Type {
        &self.variables[usize::from(var)].ty
    }
    pub(super) fn declare_var(&mut self, var_id: VarId, ty: Type) {
        assert_eq!(
            usize::from(var_id),
            self.variables.len(),
            "variable declarations not in order"
        );
        self.variables.push(VarInfo { ty });
    }
    pub(super) fn simplify_type(&self, ty: Type) -> Type {
        self.infer.simplify_type(ty)
    }
    pub(super) fn simplify_region(&self, region: Region) -> Region {
        self.infer.simplify_region(region)
    }
    pub(super) fn unify_region(&mut self, region1: Region, region2: Region, line: usize) -> Region {
        if let Some(region) = self.infer.unify_region(region1.clone(), region2.clone()) {
            region
        } else {
            let region1 = self.simplify_region(region1);
            let region2 = self.simplify_region(region2);
            self.diag
                .borrow_mut()
                .report(format!("Expected '{region1}' but got '{region2}'"), line);
            Region::Unknown
        }
    }
    pub(super) fn unify(&mut self, ty1: Type, ty2: Type, line: usize) -> Type {
        if let Some(ty) = self.infer.unify_ty(ty1.clone(), ty2.clone()) {
            ty
        } else {
            let ty1 = self.simplify_type(ty1);
            let ty2 = self.simplify_type(ty2);
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
    fn validate_main(&mut self, program: &res::Program) {
        let Some(main) = program
            .functions
            .iter()
            .position(|f| f.name.content == "main")
        else {
            return self.diag.borrow_mut().report(
                "Missing main".to_string(),
                program
                    .functions
                    .last()
                    .map(|function| function.name.line)
                    .unwrap_or(1),
            );
        };
        let main = &program.functions[main];
        if main.generics.as_ref().is_some_and(|g| g.names.is_empty()) {
            self.diag
                .borrow_mut()
                .report("'main' should not be generic".to_string(), main.line);
        }
        if !main.params.is_empty() {
            self.diag
                .borrow_mut()
                .report("'main' should have no parameters".to_string(), main.line);
        }
        if !matches!(main.return_type.kind, res::TypeKind::Unit) {
            self.diag.borrow_mut().report(
                "'main' should have '()' as return type".to_string(),
                main.line,
            );
        }
    }
    pub(super) fn lower_region(&self, region: res::Region) -> Region {
        Lower::new(&self.generics, &self.diag).lower_region(&region)
    }
    pub(super) fn lower_type(&self, ty: res::Type) -> Type {
        Lower::new(&self.generics, &self.diag).lower_type(&ty)
    }
    pub(super) fn check_function(&mut self, id: FunctionId, f: res::Function) -> Function {
        self.generics
            .clone_from(&self.function_generic_kinds[usize::from(id)]);
        let FunctionType {
            resource: _,
            params,
            return_type,
        } = self.signature_of_function(id).skip();
        let params = f
            .params
            .into_iter()
            .zip(params)
            .map(|(param, ty)| {
                self.declare_var(param.var.1, ty.clone());
                typed_ast::Param {
                    name: Ident {
                        content: param.var.0,
                        line: param.line,
                    },
                    var: param.var.1,
                    ty,
                }
            })
            .collect::<Vec<_>>();
        let body = self.check_expr(f.body, Some(*return_type));
        let unsolved_lines = self.infer.unsolved_var_lines();
        let body = if !unsolved_lines.is_empty() {
            for line in self.infer.unsolved_var_lines() {
                self.diag
                    .borrow_mut()
                    .report("type annotations needed".to_string(), line);
            }
            body
        } else {
            let mut body = body;
            TypeSubst::new(&mut self.infer).subst_expr(&mut body);
            body
        };
        self.variables.clear();
        self.infer.clear();
        let generics = std::mem::take(&mut self.generics)
            .into_iter()
            .zip(f.generics.into_iter().flat_map(|generics| generics.names))
            .map(|(kind, name)| GenericParam { name, kind })
            .collect::<Vec<_>>();
        Function {
            name: f.name,
            generics,
            params,
            return_type: body.ty.clone(),
            body,
        }
    }
    pub fn check(mut self, program: res::Program) -> Result<typed_ast::Program, TypeError> {
        self.validate_main(&program);
        let mut functions = Vec::new();
        for (function_index, function) in program.functions.into_iter().enumerate() {
            functions.push(self.check_function(FunctionId::new(function_index), function));
        }
        if !self.diag.into_inner().finish() {
            Ok(typed_ast::Program { functions })
        } else {
            Err(TypeError)
        }
    }
}
