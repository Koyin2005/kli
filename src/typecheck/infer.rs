use crate::{
    CtxtRef,
    src_loc::SrcLoc,
    types::{GenericArg, GenericArgs, IntegerKind, Type, TypeKind, TypeMap, visit::Visit},
};
enum UnifyError {
    OccursCheck,
    NoMatch,
}
#[derive(Debug)]
pub struct TypeVarInfo<'ctxt> {
    ty: Option<Type<'ctxt>>,
    loc: SrcLoc,
}
pub struct TypeInfer<'ctxt> {
    type_vars: Vec<TypeVarInfo<'ctxt>>,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> TypeInfer<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            ctxt,
            type_vars: Vec::new(),
        }
    }
    pub fn clear(&mut self) {
        self.type_vars.clear();
    }
    pub fn fresh_ty(&mut self, loc: SrcLoc) -> usize {
        let next_var = self.type_vars.len();
        self.type_vars.push(TypeVarInfo { ty: None, loc });
        next_var
    }
    fn occurs_check(&self, var: usize, ty: Type<'ctxt>) -> bool {
        struct VarFinder<'i, 'ctxt> {
            var: usize,
            found: bool,
            infer: &'i TypeInfer<'ctxt>,
        }
        impl<'ctxt> Visit<'ctxt> for VarFinder<'_, 'ctxt> {
            fn visit_type(&mut self, ty: Type<'ctxt>) {
                if self.found {
                    return;
                }

                let (&TypeKind::IntVar(ty_var) | &TypeKind::Infer(ty_var)) = ty.kind() else {
                    self.super_visit_type(ty);
                    return;
                };
                if self.var == ty_var {
                    self.found = true;
                } else if let Some(ty) = self.infer.type_vars[ty_var].ty {
                    self.visit_type(ty);
                }
            }
        }
        let mut check = VarFinder {
            var,
            found: false,
            infer: self,
        };
        check.visit_type(ty);
        check.found
    }
    pub fn unsolved_locs(&self) -> Vec<SrcLoc> {
        self.type_vars
            .iter()
            .filter_map(|var| var.ty.is_none().then_some(var.loc))
            .collect()
    }
    pub fn simplify_type(&self, ty: Type<'ctxt>) -> Type<'ctxt> {
        let Ok(ty) = Simplify(self).map_type(ty);
        ty
    }
    pub fn unify_generic_args(
        &mut self,
        args1: GenericArgs<'ctxt>,
        args2: GenericArgs<'ctxt>,
    ) -> Option<GenericArgs<'ctxt>> {
        if args1.len() != args2.len() {
            return None;
        }
        args1
            .into_iter()
            .zip(args2)
            .map(|(arg1, arg2)| Some(GenericArg(self.unify_ty(arg1.0, arg2.0)?)))
            .collect::<Option<GenericArgs>>()
    }
    fn unify_var_ty(&mut self, var: usize, ty: Type<'ctxt>) -> Result<Type<'ctxt>, UnifyError> {
        if self.occurs_check(var, ty) {
            return Err(UnifyError::OccursCheck);
        }
        match &mut self.type_vars[var] {
            TypeVarInfo {
                ty: Some(entry), ..
            } => {
                let entry = *entry;
                let ty = self.unify_ty(entry, ty);
                self.type_vars[var].ty.clone_from(&ty);
                ty
            }
            TypeVarInfo { ty: entry, .. } => {
                *entry = Some(ty);
                Some(ty)
            }
        }
        .ok_or(UnifyError::NoMatch)
    }
    pub fn unify_ty(&mut self, ty1: Type<'ctxt>, ty2: Type<'ctxt>) -> Option<Type<'ctxt>> {
        match (ty1.kind(), ty2.kind()) {
            (TypeKind::Bool, TypeKind::Bool)
            | (TypeKind::Unknown, TypeKind::Unknown)
            | (TypeKind::Char, TypeKind::Char)
            | (TypeKind::Never, TypeKind::Never)
            | (TypeKind::String, TypeKind::String) => Some(ty1),
            (&TypeKind::Param(name1, index1), &TypeKind::Param(name2, index2))
                if index1 == index2 =>
            {
                assert_eq!(name1, name2);
                Some(Type::param(self.ctxt, name1, index1))
            }
            (&TypeKind::Array(ty1), &TypeKind::Array(ty2)) => self
                .unify_ty(ty1, ty2)
                .map(|ty| Type::new_array(self.ctxt, ty)),
            (&TypeKind::Uninit(ty1), &TypeKind::Uninit(ty2)) => self
                .unify_ty(ty1, ty2)
                .map(|ty| Type::new_uninit(self.ctxt, ty)),
            (&TypeKind::Box(ty1), &TypeKind::Box(ty2)) => self
                .unify_ty(ty1, ty2)
                .map(|ty| Type::new_box(self.ctxt, ty)),

            (TypeKind::Tuple(fields1), TypeKind::Tuple(fields2))
                if fields1.len() == fields2.len() =>
            {
                let field_tys = fields1
                    .iter()
                    .zip(fields2)
                    .map(|(&field1, &field2)| self.unify_ty(field1, field2))
                    .collect::<Option<Vec<_>>>()?;

                Some(Type::tuple_from_iter(self.ctxt, field_tys))
            }
            (TypeKind::Function(function1), TypeKind::Function(function2))
                if function1.params.len() == function2.params.len() =>
            {
                let params = function1
                    .params
                    .iter()
                    .copied()
                    .zip(function2.params.iter().copied())
                    .map(|(ty1, ty2)| self.unify_ty(ty1, ty2))
                    .collect::<Option<Vec<_>>>()?;
                let return_ty = self.unify_ty(function1.return_type, function2.return_type)?;
                Some(Type::function_type(self.ctxt, params, return_ty))
            }
            (&TypeKind::Named(id1, name, ref args1), &TypeKind::Named(id2, _, ref args2))
                if id1 == id2 =>
            {
                let args = self.unify_generic_args(args1.clone(), args2.clone())?;
                Some(Type::named(self.ctxt, id1, name, args))
            }

            (&TypeKind::Int(int_kind_1), &TypeKind::Int(int_kind_2)) => {
                match (int_kind_1, int_kind_2) {
                    (IntegerKind::Signed(size1), IntegerKind::Signed(size2))
                    | (IntegerKind::Unsigned(size1), IntegerKind::Unsigned(size2))
                        if size1 == size2 =>
                    {
                        Some(ty1)
                    }

                    (IntegerKind::Signed(_) | IntegerKind::Unsigned(_), _) => None,
                }
            }
            (&TypeKind::IntVar(var), &TypeKind::Int(int))
            | (&TypeKind::Int(int), &TypeKind::IntVar(var)) => {
                let ty = Type::new_integer(self.ctxt, int);
                match self.unify_var_ty(var, ty) {
                    Ok(ty) => Some(ty),
                    Err(UnifyError::OccursCheck) => Some(ty),
                    Err(UnifyError::NoMatch) => None,
                }
            }
            (TypeKind::IntVar(var1), TypeKind::IntVar(var2)) => {
                if var1 == var2 {
                    Some(ty1)
                } else {
                    match self.unify_var_ty(*var1, ty2) {
                        Ok(ty) => Some(ty),
                        Err(UnifyError::OccursCheck) => Some(ty2),
                        Err(UnifyError::NoMatch) => None,
                    }
                }
            }
            (&TypeKind::Infer(var1), &TypeKind::Infer(var2)) if var1 == var2 => {
                Some(Type::infer_var(self.ctxt, var1))
            }
            (&TypeKind::Infer(var), _) => self.unify_var_ty(var, ty2).ok(),
            (_, &TypeKind::Infer(var)) => self.unify_var_ty(var, ty1).ok(),
            //This will fail to compile if new variants are not matched
            (
                TypeKind::Int(IntegerKind::Signed(_) | IntegerKind::Unsigned(_))
                | TypeKind::Bool
                | TypeKind::Unknown
                | TypeKind::Char
                | TypeKind::Param(..)
                | TypeKind::Function(..)
                | TypeKind::Named(..)
                | TypeKind::Never
                | TypeKind::Tuple(_)
                | TypeKind::Array(_)
                | TypeKind::String
                | TypeKind::Box(_)
                | TypeKind::Uninit(_)
                | TypeKind::IntVar(_),
                _,
            ) => None,
        }
    }
}

struct Simplify<'a, 'ctxt>(&'a TypeInfer<'ctxt>);
impl<'ctxt> TypeMap<'ctxt> for Simplify<'_, 'ctxt> {
    type Error = std::convert::Infallible;
    fn ctxt(&self) -> CtxtRef<'ctxt> {
        self.0.ctxt
    }
    fn map_type(&mut self, ty: Type<'ctxt>) -> Result<Type<'ctxt>, Self::Error> {
        let (&TypeKind::Infer(var) | &TypeKind::IntVar(var)) = ty.kind() else {
            return self.super_map_type(ty);
        };
        if let &TypeVarInfo {
            ty: Some(ty),
            loc: _,
        } = &self.0.type_vars[var]
        {
            self.map_type(ty)
        } else {
            Ok(ty)
        }
    }
}
