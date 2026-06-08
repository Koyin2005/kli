use std::{cell::RefCell, rc::Rc};

use crate::{
    diagnostics::DiagnosticReporter,
    ident::Ident,
    resolved_ast::{self as res, Builtin, FunctionId, Program, VarId},
    scheme::Scheme,
    src_loc::SrcLoc,
    typecheck::{infer::TypeInfer, lower::Lower, subst::TypeSubst},
    typed_ast::{self, Function, GenericParam, LetBinding},
    types::{FunctionType, GenericArg, GenericKind, Region, Type},
};
pub struct TypeError;
#[derive(Debug)]
struct VarInfo {
    ty: Type,
    function_scope: usize,
}
pub struct TypeCheck {
    function_generic_kinds: Vec<Vec<GenericKind>>,
    pub(super) diag: RefCell<DiagnosticReporter>,
    variables: Vec<VarInfo>,
    generics: Vec<GenericKind>,
    signatures: Vec<Scheme<FunctionType>>,
    captures: Vec<Vec<VarId>>,
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
            captures: Vec::new(),
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
            | Type::Box(_)
            | Type::Record(_) => Err(ty),
        }
    }
    pub(super) fn signature_of_builtin(&self, builtin: Builtin) -> Scheme<FunctionType> {
        let (params, return_type) = match builtin {
            Builtin::Replace => (
                vec![
                    Type::Mut(
                        Region::Param(Rc::from("r"), 0),
                        Box::new(Type::Param(Rc::from("T"), 1)),
                    ),
                    Type::Function(FunctionType::new_resource(
                        vec![Type::Param(Rc::from("T"), 1)],
                        Type::Param(Rc::from("T"), 1),
                    )),
                ],
                Type::Mut(
                    Region::Param(Rc::from("r"), 0),
                    Box::new(Type::Param(Rc::from("T"), 1)),
                ),
            ),
            Builtin::Swap => (
                vec![
                    Type::Mut(
                        Region::Param(Rc::from("r"), 0),
                        Box::new(Type::Param(Rc::from("T"), 1)),
                    ),
                    Type::Param(Rc::from("T"), 1),
                ],
                Type::Param(Rc::from("T"), 1),
            ),
            Builtin::DestroyString => (vec![Type::String], Type::Unit),
            Builtin::AllocBox => (
                vec![Type::Param(Rc::from("T"), 0)],
                (Type::Box(Box::new(Type::Param(Rc::from("T"), 0)))),
            ),
            Builtin::DeallocBox => (
                vec![Type::Box(Box::new(Type::Param(Rc::from("T"), 0)))],
                (Type::Param(Rc::from("T"), 0)),
            ),
            Builtin::DerefBox => {
                let r_param = Region::Param(Rc::from("r"), 0);
                let t_param = Type::Param(Rc::from("T"), 1);
                (
                    vec![Type::Imm(
                        r_param.clone(),
                        Box::new(Type::Box(Box::new(t_param.clone()))),
                    )],
                    (Type::Imm(r_param, Box::new(t_param))),
                )
            }
            Builtin::DerefBoxMut => {
                let r_param = Region::Param(Rc::from("r"), 0);
                let t_param = Type::Param(Rc::from("T"), 1);
                (
                    vec![Type::Mut(
                        r_param.clone(),
                        Box::new(Type::Box(Box::new(t_param.clone()))),
                    )],
                    (Type::Mut(r_param, Box::new(t_param))),
                )
            }
            Builtin::Freeze => (
                vec![Type::Mut(
                    Region::Param(Rc::from("r"), 0),
                    Box::new(Type::Param(Rc::from("T"), 1)),
                )],
                (Type::Imm(
                    Region::Param(Rc::from("r"), 0),
                    Box::new(Type::Param(Rc::from("T"), 1)),
                )),
            ),
        };
        Scheme::new(FunctionType::new_data(params, return_type))
    }
    pub(super) fn signature_of_function(&self, function: FunctionId) -> Scheme<FunctionType> {
        self.signatures[usize::from(function)].clone()
    }
    pub(super) fn instantiate_builtin_args(
        &mut self,
        builtin: Builtin,
        loc: SrcLoc,
    ) -> Vec<GenericArg> {
        match builtin {
            Builtin::AllocBox | Builtin::DeallocBox => {
                vec![GenericArg::Type(self.fresh_ty(loc))]
            }
            Builtin::DerefBox
            | Builtin::DerefBoxMut
            | Builtin::Freeze
            | Builtin::Replace
            | Builtin::Swap => {
                vec![
                    GenericArg::Region(self.fresh_region(loc.clone())),
                    GenericArg::Type(self.fresh_ty(loc)),
                ]
            }
            Builtin::DestroyString => Vec::new(),
        }
    }
    pub(super) fn with_capture_scope<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> T,
    ) -> (Vec<VarId>, T) {
        self.captures.push(Vec::new());
        let value = f(self);
        if let Some(captures) = self.captures.pop() {
            (captures, value)
        } else {
            (Vec::new(), value)
        }
    }
    pub(super) fn fresh_region(&mut self, loc: SrcLoc) -> Region {
        Region::Infer(self.infer.fresh_region(loc))
    }
    pub(super) fn fresh_ty(&mut self, loc: SrcLoc) -> Type {
        Type::Infer(self.infer.fresh_ty(loc))
    }
    pub(super) fn instantiate_function_args(
        &mut self,
        function: FunctionId,
        loc: SrcLoc,
    ) -> Vec<GenericArg> {
        self.function_generic_kinds[usize::from(function)]
            .iter()
            .map(|kind| match *kind {
                GenericKind::Region => {
                    GenericArg::Region(Region::Infer(self.infer.fresh_region(loc.clone())))
                }
                GenericKind::Type => {
                    GenericArg::Type(Type::Infer(self.infer.fresh_ty(loc.clone())))
                }
            })
            .collect()
    }
    pub(super) fn var_type(&self, var: VarId) -> &Type {
        &self.variables[usize::from(var)].ty
    }
    pub(super) fn capture(&mut self, var: VarId) -> bool {
        let function = self.variables[usize::from(var)].function_scope;
        let current = self.captures.len();
        if function < current {
            for capture in self.captures.iter_mut().rev().take(current - function) {
                capture.push(var);
            }
            true
        } else {
            false
        }
    }
    pub(super) fn declare_var(&mut self, var_id: VarId, ty: Type) {
        assert_eq!(
            usize::from(var_id),
            self.variables.len(),
            "variable declarations not in order"
        );
        self.variables.push(VarInfo {
            ty,
            function_scope: self.captures.len(),
        });
    }
    pub(super) fn simplify_type(&self, ty: Type) -> Type {
        self.infer.simplify_type(ty)
    }
    pub(super) fn unify(&mut self, ty1: Type, ty2: Type, loc: SrcLoc) -> Type {
        if let Some(ty) = self.infer.unify_ty(ty1.clone(), ty2.clone()) {
            ty
        } else {
            let ty1 = self.simplify_type(ty1);
            let ty2 = self.simplify_type(ty2);
            self.diag
                .borrow_mut()
                .add_diagnostic(format!("Expected '{ty1}' but got '{ty2}'"), loc);
            Type::Unknown
        }
    }
    pub(super) fn unify_region(&mut self, region1: Region, region2: Region, loc: SrcLoc) -> Region {
        if let Some(region) = self.infer.unify_region(region1.clone(), region2.clone()) {
            region
        } else {
            let region1 = self.infer.simplify_region(region1);
            let region2 = self.infer.simplify_region(region2);
            self.diag
                .borrow_mut()
                .add_diagnostic(format!("Expected '{region1}' but got '{region2}'"), loc);
            Region::Unknown
        }
    }

    pub(super) fn type_annotations_needed(&self, loc: SrcLoc) {
        self.diag
            .borrow_mut()
            .add_diagnostic("type annotations needed".to_string(), loc);
    }
    fn validate_main(&mut self, program: &res::Program) {
        let Some(main) = program
            .functions
            .iter()
            .position(|f| f.name.content.as_ref() == "main")
        else {
            let loc = program
                .functions
                .last()
                .map(|function| function.body.loc.clone())
                .unwrap_or(SrcLoc::dummy());
            return self
                .diag
                .borrow_mut()
                .add_diagnostic("Missing main".to_string(), loc);
        };
        let main = &program.functions[main];
        if main.generics.as_ref().is_some_and(|g| g.names.is_empty()) {
            self.diag
                .borrow_mut()
                .add_diagnostic("'main' should not be generic".to_string(), main.loc.clone());
        }
        if !main.params.is_empty() {
            self.diag.borrow_mut().add_diagnostic(
                "'main' should have no parameters".to_string(),
                main.loc.clone(),
            );
        }
        if !matches!(main.return_type.kind, res::TypeKind::Unit) {
            self.diag.borrow_mut().add_diagnostic(
                "'main' should have '()' as return type".to_string(),
                main.loc.clone(),
            );
        }
    }
    pub(super) fn lower_region(&self, region: res::Region) -> Region {
        Lower::new(&self.generics, &self.diag).lower_region(&region)
    }
    pub(super) fn lower_type(&self, ty: res::Type) -> Type {
        Lower::new(&self.generics, &self.diag).lower_type(&ty)
    }
    pub(super) fn check_binding(&mut self, binding: res::LetBinding) -> LetBinding {
        let ty = binding.ty.map(|ty| self.lower_type(ty));
        let value = self.check_expr(binding.value, ty);
        let pattern = self.check_pattern(binding.pattern, value.ty.clone(), None);
        LetBinding { pattern, value }
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
                        loc: param.loc,
                    },
                    var: param.var.1,
                    ty,
                }
            })
            .collect::<Vec<_>>();
        let body = self.check_expr(f.body, Some(*return_type));
        let unsolved = self.infer.unsolved_locs();
        let body = if !unsolved.is_empty() {
            for line in unsolved {
                self.diag
                    .borrow_mut()
                    .add_diagnostic("type annotations needed".to_string(), line);
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
        if !self.diag.into_inner().report_all() {
            Ok(typed_ast::Program { functions })
        } else {
            Err(TypeError)
        }
    }
}
