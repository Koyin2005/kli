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
    types::{self, FieldName, FunctionSig, GenericArgs, Region, Type, lower::Lower},
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
    pub(super) fn try_unify(&self, ty1: Type, ty2: Type) -> Option<Type> {
        self.infer.borrow_mut().unify_ty(ty1, ty2)
    }
    pub(super) fn unify(&self, ty1: Type, ty2: Type, loc: SrcLoc) -> Type {
        if let Some(ty) = self.try_unify(ty1.clone(), ty2.clone()) {
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
    pub(super) fn resolve_method(
        &self,
        loc: SrcLoc,
        ty: &Type,
        method: Ident,
    ) -> Result<(DefId, GenericArgs), TypeError> {
        let (name_info, _) = match ty {
            Type::Named(id, name, args) => (Some((*id, *name, args.clone())), false),
            Type::Imm(_, ty) | Type::Mut(_, ty) => (
                ty.as_named()
                    .map(|(id, name, args)| (id, name, args.to_vec())),
                true,
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
        hint: Option<&Type>,
        lit: res::IntegerLiteral,
    ) -> (Type, u64) {
        let integer_ty = match lit.kind {
            res::IntegerLiteralKind::Implicit => {
                if let Some(&Type::UINT) = hint {
                    types::IntegerKind::Unsigned
                } else {
                    types::IntegerKind::Signed
                }
            }
            res::IntegerLiteralKind::Signed => types::IntegerKind::Signed,
            res::IntegerLiteralKind::Unsigned => types::IntegerKind::Unsigned,
        };
        let ty = Type::Int(integer_ty);
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
    pub(super) fn iterator_element(&self, ty: Type) -> Result<(IteratorType, Type), Type> {
        fn by_ref_iter(
            this: &RootCtxt<'_>,
            region: Region,
            mutable: crate::ast::Mutable,
            pointee: Type,
        ) -> Result<(IteratorType, Type), Type> {
            let pointee = this.simplify_type(pointee);
            Err(Type::reference(pointee, mutable, region))
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
            _ => Err(ty),
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
            .add_diagnostic("type annotations needed", loc);
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
pub enum Coercion {
    Equal(Type),
    NeverToAny(Type),
}
pub enum CoercionKind {
    NeverToAny(Type),
}
pub struct VisibilityError;
pub struct FunctionCtxt<'ctxt> {
    pub(super) id: DefId,
    root: &'ctxt RootCtxt<'ctxt>,
    pub(super) return_type: Type,
}
impl<'ctxt> FunctionCtxt<'ctxt> {
    pub fn new(root: &'ctxt RootCtxt<'ctxt>, id: DefId, ty: Type) -> Self {
        Self {
            id,
            root,
            return_type: ty,
        }
    }
    pub fn root(&self) -> &RootCtxt<'_> {
        self.root
    }
    pub fn ctxt(&self) -> CtxtRef<'_> {
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
    pub fn merge_ty(&self, tys: impl Iterator<Item = Type>) -> Option<Type> {
        tys.into_iter().fold(None, |acc, ty| {
            if let Some(combined_ty) = acc {
                match self.root.try_unify(combined_ty.clone(), ty.clone()) {
                    Some(ty) => Some(ty),
                    None => match (combined_ty, ty) {
                        (Type::Never, ty) | (ty, Type::Never) => Some(ty),
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
        expected: Type,
        ty: Type,
    ) -> Result<Coercion, TypeError> {
        match self
            .root
            .infer
            .borrow_mut()
            .unify_ty(expected.clone(), ty.clone())
        {
            Some(ty) => Ok(Coercion::Equal(ty)),
            None => match (expected, ty) {
                (ty, Type::Never) => Ok(Coercion::NeverToAny(ty)),
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
        for item in self.ctxt.all_items() {
            for annotation in item.annotations.iter() {
                let valid = match annotation.kind {
                    res::AnnotationKind::Copy => matches!(item.kind, res::ItemKind::TypeDef(_)),
                    res::AnnotationKind::Unsafe => matches!(item.kind, res::ItemKind::Function(_)),
                    res::AnnotationKind::LangItem(lang_item) => {
                        self.ctxt.std_lib_module().is_none_or(|std_lib| {
                            self.ctxt.ancestors(item.id).any(|parent| parent == std_lib)
                        }) && match lang_item {
                            LangItem::ArrayList
                            | LangItem::String
                            | LangItem::Box
                            | LangItem::Slice
                            | LangItem::StringSlice => {
                                matches!(item.kind, res::ItemKind::TypeDef(_))
                            }
                            LangItem::ArrayListFromRaw | LangItem::StringFromSlice => {
                                matches!(item.kind, res::ItemKind::Function(_))
                            }
                        }
                    }
                    res::AnnotationKind::Opaque => matches!(item.kind, res::ItemKind::TypeDef(_)),
                };
                if !valid {
                    self.ctxt.diag().add_diagnostic(
                        format!("Cannot use '{}'", annotation.kind_str()),
                        item.loc,
                    );
                }
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
