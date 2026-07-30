use crate::{
    index_vec::IndexVec,
    src_loc::SrcLoc,
    types::{FunctionType, GenericArg, GenericArgs, IntegerKind, RecordField, TypeKind, TypeMap},
};
#[derive(Debug)]
pub struct TypeVarInfo {
    ty: Option<TypeKind>,
    loc: SrcLoc,
}
#[derive(Default)]
pub struct TypeInfer {
    type_vars: Vec<TypeVarInfo>,
}
impl TypeInfer {
    pub fn new() -> Self {
        Self {
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
    pub fn simplify_type(&self, ty: TypeKind) -> TypeKind {
        let Ok(ty) = Simplify(self).map_type(ty);
        ty
    }
    pub fn unify_generic_args(
        &mut self,
        args1: GenericArgs,
        args2: GenericArgs,
    ) -> Option<GenericArgs> {
        if args1.len() != args2.len() {
            return None;
        }
        args1
            .into_iter()
            .zip(args2)
            .map(|(arg1, arg2)| Some(GenericArg(self.unify_ty(arg1.0, arg2.0)?)))
            .collect::<Option<GenericArgs>>()
    }
    pub fn unify_ty(&mut self, ty1: TypeKind, ty2: TypeKind) -> Option<TypeKind> {
        match (ty1, ty2) {
            (ty @ TypeKind::Int(IntegerKind::Signed), TypeKind::Int(IntegerKind::Signed))
            | (ty @ TypeKind::Int(IntegerKind::Unsigned), TypeKind::Int(IntegerKind::Unsigned))
            | (ty @ TypeKind::Bool, TypeKind::Bool)
            | (ty @ TypeKind::Unknown, TypeKind::Unknown)
            | (ty @ TypeKind::Char, TypeKind::Char)
            | (ty @ TypeKind::Byte, TypeKind::Byte)
            | (ty @ TypeKind::Never, TypeKind::Never)
            | (ty @ TypeKind::String, TypeKind::String) => Some(ty),
            (TypeKind::Param(name1, index1), TypeKind::Param(name2, index2)) if index1 == index2 => {
                assert_eq!(name1, name2);
                Some(TypeKind::Param(name1, index1))
            }
            (TypeKind::Array(ty1), TypeKind::Array(ty2)) => self
                .unify_ty(*ty1, *ty2)
                .map(|ty| TypeKind::Array(Box::new(ty))),
            (TypeKind::Box(ty1), TypeKind::Box(ty2)) => {
                self.unify_ty(*ty1, *ty2).map(|ty| TypeKind::Box(Box::new(ty)))
            }
            (TypeKind::Record(fields1), TypeKind::Record(fields2)) if fields1.len() == fields2.len() => {
                fields1
                    .into_iter()
                    .zip(fields2)
                    .map(|(field1, field2)| {
                        if field1.name == field2.name {
                            let ty = self.unify_ty(field1.ty, field2.ty)?;
                            Some(RecordField {
                                name: field1.name,
                                ty,
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Option<IndexVec<_, _>>>()
                    .map(TypeKind::Record)
            }
            (TypeKind::Tuple(fields1), TypeKind::Tuple(fields2)) if fields1.len() == fields2.len() => {
                fields1
                    .into_iter()
                    .zip(fields2)
                    .map(|(field1, field2)| self.unify_ty(field1, field2))
                    .collect::<Option<_>>()
                    .map(TypeKind::Tuple)
            }
            (TypeKind::Function(function1), TypeKind::Function(function2))
                if function1.params.len() == function2.params.len() =>
            {
                let params = function1
                    .params
                    .into_iter()
                    .zip(function2.params)
                    .map(|(ty1, ty2)| self.unify_ty(ty1, ty2))
                    .collect::<Option<Vec<_>>>()?;
                let return_ty = self.unify_ty(*function1.return_type, *function2.return_type)?;
                Some(TypeKind::Function(FunctionType {
                    params,
                    return_type: Box::new(return_ty),
                }))
            }
            (TypeKind::Named(id1, name, args1), TypeKind::Named(id2, _, args2)) if id1 == id2 => {
                let args = self.unify_generic_args(args1, args2)?;
                Some(TypeKind::Named(id1, name, args))
            }
            (TypeKind::Infer(var1), TypeKind::Infer(var2)) if var1 == var2 => Some(TypeKind::Infer(var1)),
            (TypeKind::Infer(var), ty) | (ty, TypeKind::Infer(var)) => match &mut self.type_vars[var] {
                TypeVarInfo {
                    ty: Some(entry), ..
                } => {
                    let entry = entry.clone();
                    let ty = self.unify_ty(entry, ty);
                    self.type_vars[var].ty.clone_from(&ty);
                    ty
                }
                TypeVarInfo { ty: entry, .. } => {
                    *entry = Some(ty.clone());
                    Some(ty)
                }
            },
            //This will fail to compile if new variants are not matched
            (
                TypeKind::Int(IntegerKind::Signed | IntegerKind::Unsigned)
                | TypeKind::Bool
                | TypeKind::Unknown
                | TypeKind::Char
                | TypeKind::Param(..)
                | TypeKind::Function(..)
                | TypeKind::Byte
                | TypeKind::Record(..)
                | TypeKind::Named(..)
                | TypeKind::Never
                | TypeKind::Tuple(_)
                | TypeKind::Array(_)
                | TypeKind::String
                | TypeKind::Box(_),
                _,
            ) => None,
        }
    }
}

struct Simplify<'a>(&'a TypeInfer);
impl TypeMap for Simplify<'_> {
    type Error = std::convert::Infallible;
    fn map_type(&mut self, ty: TypeKind) -> Result<TypeKind, Self::Error> {
        let TypeKind::Infer(var) = ty else {
            return self.super_map_type(ty);
        };
        if let TypeVarInfo {
            ty: Some(ty),
            loc: _,
        } = &self.0.type_vars[var]
        {
            self.map_type(ty.clone())
        } else {
            Ok(ty)
        }
    }
}
