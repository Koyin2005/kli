use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
};

use crate::{
    Symbol,
    collect::CtxtRef,
    def_ids::DefId,
    ident::Ident,
    resolved_ast::{self as res, VarId},
    src_loc::SrcLoc,
    typecheck::{infer::TypeInfer, subst::TypeSubst},
    typed_ast::{self, Function, IteratorType, LetBinding},
    types::{FieldName, FunctionSig, GenericArgs, Region, Type, lower::Lower},
};
pub struct RootCtxt<'ctxt> {
    id: DefId,
    infer: RefCell<TypeInfer>,
    ctxt: CtxtRef<'ctxt>,
    variables: RefCell<HashMap<VarId, VarInfo>>,
    functions: RefCell<Vec<(DefId, Function)>>,
}
impl<'ctxt> RootCtxt<'ctxt> {
    pub fn new(id: DefId, ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            id,
            infer: Default::default(),
            ctxt,
            functions: Default::default(),
            variables: Default::default(),
        }
    }
    pub fn ctxt(&self) -> CtxtRef<'_> {
        self.ctxt
    }

    pub(super) fn declare_var(&self, var_id: VarId, ty: Type, name: Symbol) {
        self.variables
            .borrow_mut()
            .insert(var_id, VarInfo { name, ty });
    }

    fn lower(&self) -> Lower<'_> {
        Lower::new(self.ctxt, self.id, Some(&self.infer))
    }
    pub(super) fn lower_region(&self, region: &res::Region) -> Region {
        self.lower().lower_region(region)
    }
    pub(super) fn lower_type(&self, ty: &res::Type) -> Type {
        self.lower().lower_type(ty)
    }
    pub(super) fn lower_type_name(
        &self,
        loc: SrcLoc,
        ty: res::TypeName,
        args: &res::GenericArgs,
    ) -> Type {
        self.lower().lower_type_name(loc, ty, args)
    }
    pub(super) fn lower_generic_args_for(
        &self,
        id: DefId,
        loc: SrcLoc,
        args: &res::GenericArgs,
    ) -> GenericArgs {
        self.lower().lower_generic_args(id, loc, args)
    }
    pub(super) fn simplify_type(&self, ty: Type) -> Type {
        self.infer.borrow().simplify_type(ty)
    }
    pub(super) fn unify(&self, ty1: Type, ty2: Type, loc: SrcLoc) -> Type {
        if let Some(ty) = self.infer.borrow_mut().unify_ty(ty1.clone(), ty2.clone()) {
            ty
        } else {
            let ty1 = self.simplify_type(ty1);
            let ty2 = self.simplify_type(ty2);
            self.ctxt
                .diag()
                .add_diagnostic(format!("Expected '{}' but got '{}'", ty1, ty2), loc);
            Type::Unknown
        }
    }
    pub(super) fn unify_region(&self, region1: Region, region2: Region, loc: SrcLoc) -> Region {
        if let Some(region) = self.infer.borrow_mut().unify_region(region1, region2) {
            region
        } else {
            let region1 = self.infer.borrow().simplify_region(region1);
            let region2 = self.infer.borrow().simplify_region(region2);
            self.ctxt
                .diag()
                .add_diagnostic(format!("Expected '{}' but got '{}'", region1, region2), loc);
            Region::Unknown
        }
    }
    pub(super) fn iterator_element(&self, ty: Type) -> Result<(IteratorType, Type), Type> {
        fn by_ref_iter(
            this: &RootCtxt<'_>,
            region: Region,
            mutable: crate::ast::Mutable,
            pointee: Type,
        ) -> Result<(IteratorType, Type), Type> {
            match this.simplify_type(pointee) {
                Type::String => Ok((IteratorType::StringIter(region, mutable), Type::Char)),
                pointee => Err(Type::reference(pointee, mutable, region)),
            }
        }
        match ty {
            Type::Imm(region, pointee) => {
                by_ref_iter(self, region, crate::ast::Mutable::Immutable, *pointee)
            }
            Type::Mut(region, pointee) => {
                by_ref_iter(self, region, crate::ast::Mutable::Mutable, *pointee)
            }
            Type::Infer(var) => match self.simplify_type(Type::Infer(var)) {
                Type::Infer(_) => Err(ty),
                ty => self.iterator_element(ty),
            },
            Type::Bool
            | Type::Unknown
            | Type::Int
            | Type::Char
            | Type::Param(..)
            | Type::Unit
            | Type::String
            | Type::Function(_)
            | Type::Record(_)
            | Type::RawPointer(_)
            | Type::Byte
            | Type::Array(..)
            | Type::Named(..) => Err(ty),
        }
    }

    pub(super) fn fresh_ty(&self, loc: SrcLoc) -> Type {
        Type::Infer(self.infer.borrow_mut().fresh_ty(loc))
    }
    pub(super) fn check_missing_fields(
        &self,
        loc: SrcLoc,
        seen_fields: HashSet<Symbol>,
        expected_fields: impl IntoIterator<Item = FieldName>,
    ) -> Result<(), TypeError> {
        let mut had_missing = false;
        for field_name in expected_fields {
            let FieldName::Named(name) = field_name else {
                self.ctxt()
                    .diag()
                    .add_diagnostic(format!("Missing field '{}'", field_name), loc);
                had_missing = true;
                continue;
            };
            if !seen_fields.contains(&name) {
                self.ctxt()
                    .diag()
                    .add_diagnostic(format!("Missing field '{}'", field_name), loc);
                had_missing = true;
            }
        }
        if had_missing { Err(TypeError) } else { Ok(()) }
    }
    pub fn expect_ty_error(&self, kind: &str, ty: &Type, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic(format!("Expected {kind} type but got '{}'", ty), loc);
    }
    pub(super) fn type_annotations_needed(&self, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic("type annotations needed".to_string(), loc);
    }

    pub(super) fn var_type(&self, var: VarId) -> Type {
        self.variables.borrow()[&var].ty.clone()
    }
    pub(super) fn var_name(&self, var: VarId) -> Symbol {
        self.variables.borrow()[&var].name
    }

    pub(super) fn non_deref_error(&self, ty: &Type, loc: SrcLoc) -> Type {
        self.ctxt
            .diag()
            .add_diagnostic(format!("Cannot deref '{}'", ty), loc);
        Type::Unknown
    }
}
pub struct FunctionCtxt<'ctxt> {
    pub(super) id: DefId,
    root: &'ctxt RootCtxt<'ctxt>,
}
impl<'ctxt> FunctionCtxt<'ctxt> {
    pub fn new(root: &'ctxt RootCtxt<'ctxt>, id: DefId) -> Self {
        Self { id, root }
    }
    pub fn root(&self) -> &RootCtxt<'_> {
        self.root
    }
    pub fn ctxt(&self) -> CtxtRef<'_> {
        self.root.ctxt
    }
    pub(super) fn check_binding(&self, binding: &res::LetBinding) -> LetBinding {
        let ty = binding.ty.as_ref().map(|ty| self.root().lower_type(ty));
        let value = self.check_expr(&binding.value, ty);
        let pattern = self.check_pattern(&binding.pattern, value.ty.clone(), None);
        LetBinding { pattern, value }
    }
    pub(super) fn check_field_visibility(
        &self,
        field_id: DefId,
        loc: SrcLoc,
    ) -> Result<(), TypeError> {
        if self.ctxt().same_module(field_id, self.id) {
            return Ok(());
        }
        let ty_id = self.ctxt().expect_parent(field_id);
        if !self.ctxt().is_opaque(ty_id) {
            return Ok(());
        }
        let name = self.ctxt().expect_ident(field_id).symbol;
        self.ctxt()
            .diag()
            .add_diagnostic(format!("Cannot access '{}'", name), loc);
        Err(TypeError)
    }
}
pub struct TypeError;
#[derive(Debug)]
struct VarInfo {
    name: Symbol,
    ty: Type,
}
pub struct TypeCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
}

impl<'ctxt> TypeCheck<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self { ctxt }
    }
    pub(super) fn ctxt(&self) -> CtxtRef<'_> {
        self.ctxt
    }
    fn validate_main(&self) {
        let Some((main_id, main)) = self.ctxt.main_function() else {
            let loc = SrcLoc::dummy();
            self.ctxt
                .diag()
                .add_diagnostic("Missing main".to_string(), loc);
            return;
        };
        if !self.ctxt.generics(main_id).is_empty() {
            self.ctxt()
                .diag()
                .add_diagnostic("'main' should not be generic".to_string(), main.name.loc);
        }
        let signature = self.ctxt.signature_of(main_id).skip();
        if !signature.params.is_empty() {
            self.ctxt().diag().add_diagnostic(
                "'main' should have no parameters".to_string(),
                main.name.loc,
            );
        }
        if !matches!(signature.return_type, Type::Unit) {
            self.ctxt().diag().add_diagnostic(
                "'main' should have '()' as return type".to_string(),
                main.name.loc,
            );
        }
    }
    pub(super) fn check_function(
        func_ctxt: &mut FunctionCtxt,
        extra_params: Vec<(Ident, Type)>,
        sig: FunctionSig,
        params: &[res::Param],
        body: Option<&res::Expr>,
    ) {
        let FunctionSig {
            params: param_tys,
            return_type,
        } = sig;
        let params = params
            .iter()
            .zip(param_tys)
            .map(|(param, ty)| {
                func_ctxt
                    .root()
                    .declare_var(param.var.1, ty.clone(), param.var.0);
                typed_ast::Param {
                    name: param.var.ident(param.loc),
                    var: Some(param.var.1),
                    ty,
                }
            })
            .collect::<Vec<_>>();
        let body = if let Some(body) = body {
            let body = func_ctxt.check_expr(body, Some(return_type.clone()));
            Some(body)
        } else {
            None
        };
        let params = {
            let mut complete_params = extra_params
                .into_iter()
                .map(|(name, ty)| typed_ast::Param {
                    name,
                    var: None,
                    ty,
                })
                .collect::<Vec<_>>();
            complete_params.extend(params);
            complete_params
        };
        let function = Function {
            params,
            return_type,
            body,
        };
        func_ctxt
            .root()
            .functions
            .borrow_mut()
            .push((func_ctxt.id, function));
    }
    fn validate_types_non_recursive(&self) {
        for item in self.ctxt.all_items() {
            if let res::ItemKind::TypeDef(ref type_def) = item.kind
                && self.ctxt.is_type_recursive(item.id)
            {
                self.ctxt.diag().add_diagnostic(
                    format!(
                        "recursive type '{}' without indirection",
                        type_def.name.symbol
                    ),
                    type_def.name.loc,
                );
            }
        }
    }
    fn check_function_item(
        &self,
        functions: &mut BTreeMap<DefId, Function>,
        id: DefId,
        function: &res::Function,
    ) {
        let root_ctxt = RootCtxt::new(id, self.ctxt);
        Self::check_function(
            &mut FunctionCtxt::new(&root_ctxt, id),
            Vec::new(),
            self.ctxt().signature_of(id).skip(),
            &function.params,
            function.body.as_deref(),
        );
        for (id, mut function) in root_ctxt.functions.into_inner() {
            let unsolved = root_ctxt.infer.borrow().unsolved_locs();
            if !unsolved.is_empty() {
                for line in unsolved {
                    self.ctxt
                        .diag()
                        .add_diagnostic("type annotations needed".to_string(), line);
                }
            } else {
                TypeSubst::new(&mut root_ctxt.infer.borrow_mut()).subst_function(&mut function);
            }
            functions.insert(id, function);
        }
    }
    pub fn check(self) -> Result<typed_ast::Program, TypeError> {
        self.validate_main();
        self.validate_types_non_recursive();
        let mut functions = BTreeMap::new();
        for item in self.ctxt.all_items() {
            let Some(function) = item.function_def() else {
                continue;
            };
            self.check_function_item(&mut functions, item.id, function);
        }
        if !self.ctxt.diag().report_all() {
            Ok(typed_ast::Program { functions })
        } else {
            Err(TypeError)
        }
    }
}
