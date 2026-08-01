use crate::{
    CtxtRef,
    src_loc::SrcLoc,
    types::{GenericArg, GenericArgs, IntegerKind, Type, TypeKind, TypeMap},
};
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
    fn unify_var_ty(&mut self, var: usize, ty: Type<'ctxt>) -> Option<Type<'ctxt>> {
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
    }
    pub fn unify_ty(&mut self, ty1: Type<'ctxt>, ty2: Type<'ctxt>) -> Option<Type<'ctxt>> {
        match (ty1.kind(), ty2.kind()) {
            (TypeKind::Int(IntegerKind::Signed), TypeKind::Int(IntegerKind::Signed))
            | (TypeKind::Int(IntegerKind::Unsigned), TypeKind::Int(IntegerKind::Unsigned))
            | (TypeKind::Bool, TypeKind::Bool)
            | (TypeKind::Unknown, TypeKind::Unknown)
            | (TypeKind::Char, TypeKind::Char)
            | (TypeKind::Byte, TypeKind::Byte)
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
            (&TypeKind::RawDynArray(ty1), &TypeKind::RawDynArray(ty2)) => self
                .unify_ty(ty1, ty2)
                .map(|ty| Type::new_raw_dyn_array(self.ctxt, ty)),
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
            (&TypeKind::Infer(var1), &TypeKind::Infer(var2)) if var1 == var2 => {
                Some(Type::infer_var(self.ctxt, var1))
            }
            (&TypeKind::Infer(var), _) => self.unify_var_ty(var, ty2),
            (_, &TypeKind::Infer(var)) => self.unify_var_ty(var, ty1),
            //This will fail to compile if new variants are not matched
            (
                TypeKind::Int(IntegerKind::Signed | IntegerKind::Unsigned)
                | TypeKind::Bool
                | TypeKind::Unknown
                | TypeKind::Char
                | TypeKind::Param(..)
                | TypeKind::Function(..)
                | TypeKind::Byte
                | TypeKind::Named(..)
                | TypeKind::Never
                | TypeKind::Tuple(_)
                | TypeKind::Array(_)
                | TypeKind::String
                | TypeKind::Box(_)
                | TypeKind::RawDynArray(_),
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
        let &TypeKind::Infer(var) = ty.kind() else {
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
