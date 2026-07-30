use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
};

use crate::{
    Symbol,
    collect::CtxtRef,
    def_ids::DefId,
    ident::Ident,
    lang_items::LangItem,
    resolved_ast::{self as res, Node, VarId},
    src_loc::SrcLoc,
    typecheck::{infer::TypeInfer, subst::TypeSubst},
    typed_ast::{self, Function, IteratorType, LetBinding},
    types::{self, FieldName, FunctionSig, GenericArgs, TypeKind, lower::Lower},
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
    pub fn ctxt(&'_ self) -> CtxtRef<'ctxt> {
        self.ctxt
    }

    pub(super) fn declare_var(&self, var_id: VarId, ty: TypeKind, name: Symbol) {
        self.variables
            .borrow_mut()
            .insert(var_id, VarInfo { name, ty });
    }

    fn lower(&self) -> Lower<'_, 'ctxt> {
        Lower::new(self.ctxt, self.id, Some(&self.infer))
    }
    pub(super) fn lower_type(&self, ty: &res::Type) -> TypeKind {
        self.lower().lower_type(ty)
    }
    pub(super) fn lower_type_name(
        &self,
        loc: SrcLoc,
        ty: res::TypeName,
        args: &res::GenericArgs,
    ) -> TypeKind {
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
    pub(super) fn simplify_type(&self, ty: TypeKind) -> TypeKind {
        self.infer.borrow().simplify_type(ty)
    }
    pub(super) fn try_unify(&self, ty1: TypeKind, ty2: TypeKind) -> Option<TypeKind> {
        self.infer.borrow_mut().unify_ty(ty1, ty2)
    }
    pub(super) fn unify(&self, ty1: TypeKind, ty2: TypeKind, loc: SrcLoc) -> TypeKind {
        if let Some(ty) = self.try_unify(ty1.clone(), ty2.clone()) {
            ty
        } else {
            let ty1 = self.simplify_type(ty1);
            let ty2 = self.simplify_type(ty2);
            self.ctxt
                .diag()
                .add_diagnostic(format!("Expected '{}' but got '{}'", ty1, ty2), loc);
            TypeKind::Unknown
        }
    }
    pub(super) fn resolve_method(
        &self,
        loc: SrcLoc,
        ty: &TypeKind,
        method: Ident,
    ) -> Result<(DefId, GenericArgs), TypeError> {
        let (name_info, _) = match ty {
            TypeKind::Named(id, name, args) => (Some((*id, *name, args.clone())), false),
            TypeKind::Array(ty) => (
                self.ctxt
                    .lang_items()
                    .get(LangItem::Array)
                    .map(|id| (id, Symbol::ARRAY, GenericArgs::from_type((**ty).clone()))),
                false,
            ),
            _ => (None, false),
        };
        let ctxt = self.ctxt();
        let impl_ =
            name_info.and_then(|(id, _, args)| ctxt.impl_for(id).map(|impl_| (impl_, args)));
        let method_info = impl_.and_then(|(impl_, args)| {
            impl_
                .methods
                .iter()
                .find_map(|&id| {
                    if ctxt.ident(id)?.symbol == method.symbol {
                        Some(id)
                    } else {
                        None
                    }
                })
                .map(|impl_| (impl_, args))
        });
        let Some((id, args)) = method_info else {
            self.ctxt().diag().add_diagnostic(
                format!("'{}' does not have method '{}'", ty, method.symbol),
                loc,
            );
            return Err(TypeError);
        };
        Ok((id, args))
    }
    pub(super) fn check_int_lit(
        &self,
        loc: SrcLoc,
        hint: Option<&TypeKind>,
        lit: res::IntegerLiteral,
    ) -> (TypeKind, u64) {
        let integer_ty = match lit.kind {
            res::IntegerLiteralKind::Implicit => {
                if let Some(&TypeKind::UINT) = hint {
                    types::IntegerKind::Unsigned
                } else {
                    types::IntegerKind::Signed
                }
            }
            res::IntegerLiteralKind::Signed => types::IntegerKind::Signed,
            res::IntegerLiteralKind::Unsigned => types::IntegerKind::Unsigned,
        };
        let ty = TypeKind::Int(integer_ty);
        let value = lit.value;
        if let types::IntegerKind::Signed = integer_ty
            && value > i64::MAX as u64
        {
            self.ctxt.diag().add_diagnostic(
                format!("Integer literal '{value}' too large for '{}'", ty),
                loc,
            );
        }
        (ty, value)
    }
    pub(super) fn iterator_element(
        &self,
        ty: TypeKind,
    ) -> Result<(IteratorType, TypeKind), TypeKind> {
        match ty {
            TypeKind::Infer(var) => match self.simplify_type(TypeKind::Infer(var)) {
                TypeKind::Infer(_) => Err(ty),
                ty => self.iterator_element(ty),
            },
            _ => Err(ty),
        }
    }

    pub(super) fn fresh_ty(&self, loc: SrcLoc) -> TypeKind {
        TypeKind::Infer(self.infer.borrow_mut().fresh_ty(loc))
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
    pub fn expect_ty_error(&self, kind: &str, ty: &TypeKind, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic(format!("Expected {kind} type but got '{}'", ty), loc);
    }
    pub(super) fn type_annotations_needed(&self, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic("type annotations needed", loc);
    }

    pub(super) fn var_type(&self, var: VarId) -> TypeKind {
        self.variables.borrow()[&var].ty.clone()
    }
    pub(super) fn var_name(&self, var: VarId) -> Symbol {
        self.variables.borrow()[&var].name
    }
}
pub enum Coercion {
    Equal(TypeKind),
    NeverToAny(TypeKind),
}
pub enum CoercionKind {
    NeverToAny(TypeKind),
}
pub struct VisibilityError;
pub struct FunctionCtxt<'root, 'ctxt> {
    pub(super) id: DefId,
    root: &'root RootCtxt<'ctxt>,
    pub(super) return_type: TypeKind,
}
impl<'root, 'ctxt> FunctionCtxt<'root, 'ctxt> {
    pub fn new(root: &'root RootCtxt<'ctxt>, id: DefId, ty: TypeKind) -> Self {
        Self {
            id,
            root,
            return_type: ty,
        }
    }
    pub fn root(&'_ self) -> &'_ RootCtxt<'ctxt> {
        self.root
    }
    pub fn ctxt(&'_ self) -> CtxtRef<'ctxt> {
        self.root.ctxt
    }
    pub fn apply_coercion(&self, coercion: Coercion, expr: typed_ast::Expr) -> typed_ast::Expr {
        match coercion {
            Coercion::Equal(_) => expr,
            Coercion::NeverToAny(ty) => typed_ast::Expr {
                ty,
                loc: expr.loc,
                kind: typed_ast::ExprKind::NeverToAny(Box::new(expr)),
            },
        }
    }
    pub fn merge_ty(&self, tys: impl Iterator<Item = TypeKind>) -> Option<TypeKind> {
        tys.into_iter().fold(None, |acc, ty| {
            if let Some(combined_ty) = acc {
                match self.root.try_unify(combined_ty.clone(), ty.clone()) {
                    Some(ty) => Some(ty),
                    None => match (combined_ty, ty) {
                        (TypeKind::Never, ty) | (ty, TypeKind::Never) => Some(ty),
                        (combined_ty, _) => Some(combined_ty),
                    },
                }
            } else {
                Some(ty)
            }
        })
    }
    pub fn unify_or_coerce(
        &self,
        loc: SrcLoc,
        expected: TypeKind,
        ty: TypeKind,
    ) -> Result<Coercion, TypeError> {
        match self
            .root
            .infer
            .borrow_mut()
            .unify_ty(expected.clone(), ty.clone())
        {
            Some(ty) => Ok(Coercion::Equal(ty)),
            None => match (expected, ty) {
                (ty, TypeKind::Never) => Ok(Coercion::NeverToAny(ty)),
                (expected, ty) => {
                    self.ctxt()
                        .diag()
                        .add_diagnostic(format!("Cannot coerce '{}' to '{}'", ty, expected), loc);
                    Err(TypeError)
                }
            },
        }
    }
    pub(super) fn check_binding(&self, binding: &res::LetBinding) -> LetBinding {
        let ty = binding.ty.as_ref().map(|ty| self.root().lower_type(ty));
        let value = self.check_expr_coerces_to(&binding.value, ty.clone());
        let pattern = self.check_pattern(&binding.pattern, value.ty.clone());
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
    ty: TypeKind,
}
pub struct TypeCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
}

impl<'ctxt> TypeCheck<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self { ctxt }
    }
    pub(super) fn ctxt(&self) -> CtxtRef<'ctxt> {
        self.ctxt
    }
    fn validate_main(&self) {
        let Some((main_id, main)) = self.ctxt.main_function() else {
            let loc = SrcLoc::dummy();
            self.ctxt.diag().add_diagnostic("Missing main", loc);
            return;
        };
        if !self.ctxt.generics(main_id).is_empty() {
            self.ctxt()
                .diag()
                .add_diagnostic("'main' should not be generic", main.name.loc);
        }
        let signature = self.ctxt.signature_of(main_id).skip();
        if !signature.params.is_empty() {
            self.ctxt()
                .diag()
                .add_diagnostic("'main' should have no parameters", main.name.loc);
        }
        if !signature.return_type.is_unit() {
            self.ctxt()
                .diag()
                .add_diagnostic("'main' should have '()' as return type", main.name.loc);
        }
    }
    pub(super) fn check_function(
        func_ctxt: &mut FunctionCtxt,
        extra_params: Vec<(Ident, TypeKind)>,
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
            let body = func_ctxt.check_expr_coerces_to(body, Some(return_type.clone()));
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
        let sig = self.ctxt().signature_of(id).skip();
        Self::check_function(
            &mut FunctionCtxt::new(&root_ctxt, id, sig.return_type.clone()),
            Vec::new(),
            sig,
            &function.params,
            function.body.as_deref(),
        );
        for (id, mut function) in root_ctxt.functions.into_inner() {
            let unsolved = root_ctxt.infer.borrow().unsolved_locs();
            if !unsolved.is_empty() {
                for line in unsolved {
                    self.ctxt
                        .diag()
                        .add_diagnostic("type annotations needed", line);
                }
            } else {
                TypeSubst::new(&mut root_ctxt.infer.borrow_mut()).subst_function(&mut function);
            }
            functions.insert(id, function);
        }
    }
    fn check_annotations(&self) {
        for (id, node) in self.ctxt.all_nodes_with_id() {
            for annotation in self.ctxt.annotations(id) {
                let valid = match annotation.kind {
                    res::AnnotationKind::Builtin => node.function_item().is_some_and(|_| {
                        let builtins = self.ctxt.builtins_module();
                        self.ctxt.ancestors(id).any(|parent| parent == builtins)
                    }),
                    res::AnnotationKind::Copy => node.is_type_def(),
                    res::AnnotationKind::Unsafe => node.is_function(),
                    res::AnnotationKind::LangItem(lang_item) => {
                        self.ctxt.std_lib_module().is_none_or(|std_lib| {
                            self.ctxt.ancestors(id).any(|parent| parent == std_lib)
                        }) && match lang_item {
                            LangItem::Array
                            | LangItem::String
                            | LangItem::Box
                            | LangItem::Slice
                            | LangItem::StringSlice => node.is_type_def(),
                            LangItem::StringFromSlice => node.is_function(),
                        }
                    }
                    res::AnnotationKind::Opaque => node.is_type_def(),
                };
                if !valid {
                    self.ctxt.diag().add_diagnostic(
                        format!("Cannot use '{}'", annotation.kind_str()),
                        node.loc(),
                    );
                }
            }

            if let Some(function) = node.function()
                && function.body.is_none()
                && self.ctxt.builtins().builtin_for(id).is_none()
            {
                self.ctxt.diag().add_diagnostic(
                    format!("'{}' must have a body", function.name.symbol),
                    function.name.loc,
                );
            }
        }
    }
    pub fn check(self) -> Result<typed_ast::Program, TypeError> {
        self.validate_main();
        self.validate_types_non_recursive();
        self.check_annotations();
        let mut functions = BTreeMap::new();
        for item in self.ctxt.all_items() {
            match &item.kind {
                res::ItemKind::Function(function) => {
                    self.check_function_item(&mut functions, item.id, function)
                }
                res::ItemKind::TypeDef(_) => {
                    let Some(impl_) = self.ctxt.impl_for(item.id) else {
                        continue;
                    };
                    for &id in &impl_.methods {
                        let Node::Method(method) = self.ctxt.node(id) else {
                            unreachable!()
                        };
                        self.check_function_item(&mut functions, id, &method.function);
                    }
                }
                _ => (),
            }
        }
        if !self.ctxt.diag().report_all() {
            Ok(typed_ast::Program { functions })
        } else {
            Err(TypeError)
        }
    }
}
