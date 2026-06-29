use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use crate::{
    collect::CtxtRef,
    ident::Ident,
    resolved_ast::{self as res, Builtin, DefId, VarId},
    scheme::Scheme,
    src_loc::SrcLoc,
    typecheck::{infer::TypeInfer, subst::TypeSubst},
    typed_ast::{self, Function, IteratorType, LetBinding},
    types::{self, FunctionSig, GenericArg, GenericKind, Region, Type, lower::Lower},
};
pub struct TypeError;
#[derive(Debug)]
struct VarInfo {
    name: Rc<str>,
    ty: Type,
    function_scope: usize,
}
struct Generics {
    generics: Vec<GenericKind>,
}
fn lower_generics(generics: Option<&res::Generics>) -> Generics {
    Generics {
        generics: match generics {
            None => Vec::new(),
            Some(ref generics) => generics
                .kinds
                .iter()
                .map(|kind| match kind {
                    res::GenericKind::Region => GenericKind::Region,
                    res::GenericKind::Type => GenericKind::Type,
                })
                .collect::<Vec<_>>(),
        },
    }
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
        match ty {
            Type::Imm(_, _) | Type::Mut(_, _) => {
                let (mutable, region, pointee) =
                    ty.as_reference_type().expect("Should be a reference");
                match self.simplify_type(pointee.clone()) {
                    Type::List(element) => Ok((
                        IteratorType::ArrayListRef(region.clone(), mutable, (*element).clone()),
                        Type::reference(*element, mutable, region.clone()),
                    )),
                    Type::String => Ok((
                        IteratorType::StringIter(region.clone(), mutable),
                        Type::Char,
                    )),
                    _ => Err(ty),
                }
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
            | Type::Option(_)
            | Type::Function(_)
            | Type::Box(_)
            | Type::Record(_)
            | Type::RawPointer(_)
            | Type::Byte
            | Type::Array(..) => Err(ty),
        }
    }
    fn generic_params_for_builtin(&self, builtin: Builtin) -> Vec<types::GenericParam> {
        match builtin {
            Builtin::Allocate
            | Builtin::PtrRead
            | Builtin::PtrWrite
            | Builtin::Deallocate
            | Builtin::BoxFromRaw
            | Builtin::BoxIntoRaw => vec![types::GenericParam {
                name: Rc::from("T"),
                kind: GenericKind::Type,
            }],
            Builtin::Freeze | Builtin::RefFromRaw(_) | Builtin::RefIntoRaw(_) => vec![
                types::GenericParam {
                    name: Rc::from("r"),
                    kind: GenericKind::Region,
                },
                types::GenericParam {
                    name: Rc::from("T"),
                    kind: GenericKind::Type,
                },
            ],
        }
    }
    pub(super) fn ctxt(&self) -> CtxtRef {
        self.ctxt
    }
    pub(super) fn signature_of_builtin(&self, builtin: Builtin) -> Scheme<FunctionSig> {
        let generics = self.generic_params_for_builtin(builtin);
        let ty_param = |index: usize| {
            let param = &generics[index];
            assert_eq!(param.kind, GenericKind::Type);
            Type::Param(param.name.clone(), index)
        };
        let region_param = |index: usize| {
            let param = &generics[index];
            assert_eq!(param.kind, GenericKind::Region);
            Region::Param(param.name.clone(), index)
        };
        let (params, return_type) = match builtin {
            Builtin::PtrRead => (vec![Type::pointer(ty_param(0))], ty_param(0)),
            Builtin::PtrWrite => (vec![Type::pointer(ty_param(0)), ty_param(0)], Type::Unit),
            Builtin::RefFromRaw(mutable) => (
                vec![Type::pointer(ty_param(1))],
                ty_param(1).reference(mutable, region_param(0)),
            ),
            Builtin::RefIntoRaw(mutable) => (
                vec![ty_param(1).reference(mutable, region_param(0))],
                Type::pointer(ty_param(1)),
            ),
            Builtin::BoxFromRaw => (
                vec![Type::pointer(ty_param(0))],
                Type::Box(Box::new(ty_param(0))),
            ),
            Builtin::BoxIntoRaw => (
                vec![Type::Box(Box::new(ty_param(0)))],
                Type::pointer(ty_param(0)),
            ),
            Builtin::Allocate => (vec![Type::Int], (Type::pointer(ty_param(0)))),
            Builtin::Deallocate => (vec![Type::pointer(ty_param(0))], (Type::Unit)),
            Builtin::Freeze => (
                vec![Type::Mut(region_param(0), Box::new(ty_param(1)))],
                (Type::Imm(region_param(0), Box::new(ty_param(1)))),
            ),
        };
        Scheme::new(FunctionSig::new(params, return_type))
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
    pub(super) fn var_name(&self, var: VarId) -> Rc<str> {
        self.variables.borrow()[usize::from(var)].name.clone()
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
    pub(super) fn non_deref_error(&self, ty: Type, loc: SrcLoc) -> Type {
        self.ctxt
            .diag()
            .add_diagnostic(format!("Cannot deref '{ty}'"), loc);
        Type::Unknown
    }
    pub(super) fn declare_var(&self, var_id: VarId, ty: Type, name: Rc<str>) {
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
                .add_diagnostic(format!("Expected '{ty1}' but got '{ty2}'"), loc);
            Type::Unknown
        }
    }
    pub(super) fn unify_region(&self, region1: Region, region2: Region, loc: SrcLoc) -> Region {
        if let Some(region) = self
            .infer
            .borrow_mut()
            .unify_region(region1.clone(), region2.clone())
        {
            region
        } else {
            let region1 = self.infer.borrow().simplify_region(region1);
            let region2 = self.infer.borrow().simplify_region(region2);
            self.ctxt
                .diag()
                .add_diagnostic(format!("Expected '{region1}' but got '{region2}'"), loc);
            Region::Unknown
        }
    }

    pub(super) fn type_annotations_needed(&self, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic("type annotations needed".to_string(), loc);
    }
    fn validate_main(&self) -> Result<(), TypeError> {
        let Some(main_id) = self.ctxt.main_id() else {
            let loc = SrcLoc::dummy();
            self.ctxt
                .diag()
                .add_diagnostic("Missing main".to_string(), loc);
            return Err(TypeError);
        };
        let main = self.ctxt.function_def(main_id);
        let main = main.unwrap();
        if !self.ctxt.generics(main_id).is_empty() {
            self.ctxt().diag().add_diagnostic(
                "'main' should not be generic".to_string(),
                main.name.loc.clone(),
            );
        }
        if !main.params.is_empty() {
            self.ctxt().diag().add_diagnostic(
                "'main' should have no parameters".to_string(),
                main.name.loc.clone(),
            );
        }
        if !matches!(main.return_type.kind, res::TypeKind::Unit) {
            self.ctxt().diag().add_diagnostic(
                "'main' should have '()' as return type".to_string(),
                main.name.loc.clone(),
            );
            return Err(TypeError);
        }
        if self.ctxt.parent_of(main_id).is_some() {
            self.ctxt().diag().add_diagnostic(
                "'main' should be at top level".to_string(),
                main.name.loc.clone(),
            );
            return Err(TypeError);
        }
        Ok(())
    }
    pub(super) fn current_function(&self) -> DefId {
        self.current_function.get().unwrap()
    }
    pub(super) fn lower(&self) -> Lower {
        Lower::new(self.ctxt, self.current_function())
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
        self.lower()
            .lower_generic_args(id, loc, args, Some(&mut *self.infer.borrow_mut()))
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
                self.declare_var(param.var.1, ty.clone(), param.var.0.clone());
                typed_ast::Param {
                    name: Ident {
                        content: param.var.0.clone(),
                        loc: param.loc.clone(),
                    },
                    var: param.var.1,
                    ty,
                }
            })
            .collect::<Vec<_>>();
        let body = if let Some(body) = body {
            let body = self.check_expr(body, Some((*return_type).clone()));
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
            return_type: *return_type,
            body,
        })
    }
    pub fn check(self) -> Result<typed_ast::Program, TypeError> {
        let mut functions = HashMap::new();
        for id in self.ctxt.root_ids() {
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
