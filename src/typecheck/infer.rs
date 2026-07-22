use crate::{
    index_vec::IndexVec,
    src_loc::SrcLoc,
    types::{
        FunctionType, GenericArg, GenericArgs, IntegerKind, RecordField, Region, Type, TypeMap,
    },
};
#[derive(Debug)]
pub struct TypeVarInfo {
    ty: Option<Type>,
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
    pub fn simplify_region(&self, region: Region) -> Region {
        region
    }
    pub fn simplify_type(&self, ty: Type) -> Type {
        let Ok(ty) = Simplify(self).map_type(ty);
        ty
    }
    pub fn unify_region(&mut self, region1: Region, region2: Region) -> Option<Region> {
        match (region1, region2) {
            _ => None,
        }
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
            .map(|(arg1, arg2)| {
                Some(match (arg1, arg2) {
                    (GenericArg::Type(ty1), GenericArg::Type(ty2)) => {
                        GenericArg::Type(self.unify_ty(ty1, ty2)?)
                    }
                })
            })
            .collect::<Option<GenericArgs>>()
    }
    pub fn unify_ty(&mut self, ty1: Type, ty2: Type) -> Option<Type> {
        match (ty1, ty2) {
            (ty @ Type::Int(IntegerKind::Signed), Type::Int(IntegerKind::Signed))
            | (ty @ Type::Int(IntegerKind::Unsigned), Type::Int(IntegerKind::Unsigned))
            | (ty @ Type::Bool, Type::Bool)
            | (ty @ Type::Unknown, Type::Unknown)
            | (ty @ Type::Char, Type::Char)
            | (ty @ Type::Byte, Type::Byte)
            | (ty @ Type::Never, Type::Never) => Some(ty),
            (Type::Param(name1, index1), Type::Param(name2, index2)) if index1 == index2 => {
                assert_eq!(name1, name2);
                Some(Type::Param(name1, index1))
            }
            (Type::Array(ty1, count1), Type::Array(ty2, count2)) if count1 == count2 => self
                .unify_ty(*ty1, *ty2)
                .map(|ty| Type::Array(Box::new(ty), count1)),
            (Type::RawPointer(ty1), Type::RawPointer(ty2)) => self
                .unify_ty(*ty1, *ty2)
                .map(|ty| Type::RawPointer(Box::new(ty))),
            (Type::Record(fields1), Type::Record(fields2)) if fields1.len() == fields2.len() => {
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
                    .map(Type::Record)
            }
            (Type::Tuple(fields1), Type::Tuple(fields2)) if fields1.len() == fields2.len() => {
                fields1
                    .into_iter()
                    .zip(fields2)
                    .map(|(field1, field2)| self.unify_ty(field1, field2))
                    .collect::<Option<_>>()
                    .map(Type::Tuple)
            }
            (Type::Function(function1), Type::Function(function2))
                if function1.params.len() == function2.params.len()
                    && function1.resource == function2.resource =>
            {
                let params = function1
                    .params
                    .into_iter()
                    .zip(function2.params)
                    .map(|(ty1, ty2)| self.unify_ty(ty1, ty2))
                    .collect::<Option<Vec<_>>>()?;
                let return_ty = self.unify_ty(*function1.return_type, *function2.return_type)?;
                Some(Type::Function(FunctionType {
                    resource: function1.resource,
                    params,
                    return_type: Box::new(return_ty),
                }))
            }
            (Type::Named(id1, name, args1), Type::Named(id2, _, args2)) if id1 == id2 => {
                let args = self.unify_generic_args(args1, args2)?;
                Some(Type::Named(id1, name, args))
            }
            (Type::Infer(var1), Type::Infer(var2)) if var1 == var2 => Some(Type::Infer(var1)),
            (Type::Infer(var), ty) | (ty, Type::Infer(var)) => match &mut self.type_vars[var] {
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
                Type::Int(IntegerKind::Signed | IntegerKind::Unsigned)
                | Type::Bool
                | Type::Unknown
                | Type::Char
                | Type::Param(..)
                | Type::Array(..)
                | Type::Function(..)
                | Type::Byte
                | Type::Record(..)
                | Type::RawPointer(_)
                | Type::Named(..)
                | Type::Never
                | Type::Tuple(_),
                _,
            ) => None,
        }
    }
}

struct Simplify<'a>(&'a TypeInfer);
impl TypeMap for Simplify<'_> {
    type Error = std::convert::Infallible;
    fn map_type(&mut self, ty: Type) -> Result<Type, Self::Error> {
        let Type::Infer(var) = ty else {
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
