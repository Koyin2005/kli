use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
};

use crate::{
    Symbol,
    collect::CtxtRef,
    ident::Ident,
    resolved_ast::{self as res, DefId, VarId},
    src_loc::SrcLoc,
    typecheck::{infer::TypeInfer, subst::TypeSubst},
    typed_ast::{self, Function, IteratorType, LetBinding},
    types::{FieldName, FunctionSig, GenericArg, Region, Type, lower::Lower},
};
pub struct TypeError;
#[derive(Debug)]
struct VarInfo {
    name: Symbol,
    ty: Type,
    function_scope: usize,
}
pub struct TypeCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    variables: RefCell<Vec<VarInfo>>,
    captures: RefCell<Vec<Vec<VarId>>>,
    current_function: Cell<Option<DefId>>,
    pub(super) infer: RefCell<TypeInfer>,
}

impl<'ctxt> TypeCheck<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            ctxt,
            infer: RefCell::default(),
            variables: RefCell::default(),
            captures: RefCell::default(),
            current_function: Cell::default(),
        }
    }
    pub(super) fn iterator_element(&self, ty: Type) -> Result<(IteratorType, Type), Type> {
        fn by_ref_iter(
            this: &TypeCheck<'_>,
            region: Region,
            mutable: crate::ast::Mutable,
            pointee: Type,
        ) -> Result<(IteratorType, Type), Type> {
            match this.simplify_type(pointee) {
                Type::List(element) => Ok((
                    IteratorType::ArrayListRef(region, mutable, (*element).clone()),
                    Type::reference(*element, mutable, region),
                )),
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
            | Type::List(_)
            | Type::String
            | Type::Function(_)
            | Type::Box(_)
            | Type::Record(_)
            | Type::RawPointer(_)
            | Type::Byte
            | Type::Array(..)
            | Type::Named(..) => Err(ty),
        }
    }
    pub(super) fn ctxt(&self) -> CtxtRef<'_> {
        self.ctxt
    }
    pub(super) fn with_capture_scope<T>(&self, f: impl FnOnce(&Self) -> T) -> (Vec<VarId>, T) {
        self.captures.borrow_mut().push(Vec::new());
        let value = f(self);
        if let Some(captures) = self.captures.borrow_mut().pop() {
            (captures, value)
        } else {
            (Vec::new(), value)
        }
    }
    pub(super) fn fresh_ty(&self, loc: SrcLoc) -> Type {
        Type::Infer(self.infer.borrow_mut().fresh_ty(loc))
    }
    pub(super) fn var_type(&self, var: VarId) -> Type {
        self.variables.borrow()[usize::from(var)].ty.clone()
    }
    pub(super) fn var_name(&self, var: VarId) -> Symbol {
        self.variables.borrow()[usize::from(var)].name
    }
    pub(super) fn capture(&self, var: VarId) -> bool {
        let function = self.variables.borrow()[usize::from(var)].function_scope;
        let current = self.captures.borrow().len();
        if function < current {
            for capture in self
                .captures
                .borrow_mut()
                .iter_mut()
                .rev()
                .take(current - function)
            {
                capture.push(var);
            }
            true
        } else {
            false
        }
    }
    pub(super) fn non_deref_error(&self, ty: &Type, loc: SrcLoc) -> Type {
        self.ctxt
            .diag()
            .add_diagnostic(format!("Cannot deref '{}'", ty), loc);
        Type::Unknown
    }
    pub(super) fn declare_var(&self, var_id: VarId, ty: Type, name: Symbol) {
        assert_eq!(
            usize::from(var_id),
            self.variables.borrow().len(),
            "variable declarations not in order"
        );
        self.variables.borrow_mut().push(VarInfo {
            name,
            ty,
            function_scope: self.captures.borrow().len(),
        });
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

    pub(super) fn type_annotations_needed(&self, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic("type annotations needed".to_string(), loc);
    }
    fn validate_main(&self)  {
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
        if !main.params.is_empty() {
            self.ctxt().diag().add_diagnostic(
                "'main' should have no parameters".to_string(),
                main.name.loc,
            );
        }
        if !matches!(main.return_type.kind, res::TypeKind::Unit) {
            self.ctxt().diag().add_diagnostic(
                "'main' should have '()' as return type".to_string(),
                main.name.loc,
            );
            return;
        }
    }
    pub(super) fn current_function(&self) -> DefId {
        self.current_function.get().unwrap()
    }
    fn lower(&self) -> Lower<'_> {
        Lower::new(self.ctxt, self.current_function(), Some(&self.infer))
    }
    pub(super) fn lower_region(&self, region: &res::Region) -> Region {
        self.lower().lower_region(region)
    }
    pub(super) fn lower_type(&self, ty: &res::Type) -> Type {
        self.lower().lower_type(ty)
    }
    pub(super) fn lower_generic_args_for(
        &self,
        id: DefId,
        args: &res::GenericArgs,
        loc: SrcLoc,
    ) -> Vec<GenericArg> {
        self.lower().lower_generic_args(id, loc, args)
    }
    pub(super) fn check_binding(&self, binding: &res::LetBinding) -> LetBinding {
        let ty = binding.ty.as_ref().map(|ty| self.lower_type(ty));
        let value = self.check_expr(&binding.value, ty);
        let pattern = self.check_pattern(&binding.pattern, value.ty.clone(), None);
        LetBinding { pattern, value }
    }
    pub(super) fn check_function(&self, id: DefId) -> Option<Function> {
        let res::Function { params, body, .. } = self.ctxt.function_def(id)?;
        self.current_function.set(Some(id));
        let FunctionSig {
            params: param_tys,
            return_type,
        } = self.ctxt.signature_of(id).skip();
        let params = params
            .iter()
            .zip(param_tys)
            .map(|(param, ty)| {
                self.declare_var(param.var.1, ty.clone(), param.var.0);
                typed_ast::Param {
                    name: Ident {
                        symbol: param.var.0,
                        loc: param.loc,
                    },
                    var: param.var.1,
                    ty,
                }
            })
            .collect::<Vec<_>>();
        let body = if let Some(body) = body {
            let body = self.check_expr(body, Some(return_type.clone()));
            let unsolved = self.infer.borrow().unsolved_locs();
            let body = if !unsolved.is_empty() {
                for line in unsolved {
                    self.ctxt
                        .diag()
                        .add_diagnostic("type annotations needed".to_string(), line);
                }
                body
            } else {
                let mut body = body;
                TypeSubst::new(&mut self.infer.borrow_mut()).subst_expr(&mut body);
                body
            };
            Some(body)
        } else {
            None
        };
        self.variables.borrow_mut().clear();
        self.infer.borrow_mut().clear();
        Some(Function {
            params,
            return_type,
            body,
        })
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
    fn validate_types_non_recursive(&self) -> () {
        for item in self.ctxt.all_items() {
            if let res::ItemKind::TypeDef(ref type_def) = item.kind {
                let def_id = item.id.into_def_id();
                if self.ctxt.is_type_recursive(def_id) {
                    self.ctxt.diag().add_diagnostic(
                        format!("recursive type '{}' without indirection", type_def.name.symbol),
                        type_def.name.loc,
                    );
                }
            }
        }
    }
    pub fn check(self) -> Result<typed_ast::Program, TypeError> {
        let mut functions = HashMap::new();
        let _ = self.validate_main();
        self.validate_types_non_recursive();
        for item in self.ctxt.all_items() {
            let id = item.id.0;
            let Some(function) = self.check_function(id) else {
                continue;
            };
            functions.insert(id, function);
        }
        if !self.ctxt.diag().report_all() {
            Ok(typed_ast::Program { functions })
        } else {
            Err(TypeError)
        }
    }
}
