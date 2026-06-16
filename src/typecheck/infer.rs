use crate::{
    index_vec::IndexVec,
    src_loc::SrcLoc,
    types::{FunctionType, RecordField, Region, Type, TypeMap},
};
#[derive(Debug)]
pub struct TypeVarInfo {
    ty: Option<Type>,
    loc: SrcLoc,
}
#[derive(Debug)]
pub struct RegionVarInfo {
    region: Option<Region>,
    loc: SrcLoc,
}
pub struct TypeInfer {
    pub type_vars: Vec<TypeVarInfo>,
    pub region_vars: Vec<RegionVarInfo>,
}
impl TypeInfer {
    pub fn new() -> Self {
        Self {
            type_vars: Vec::new(),
            region_vars: Vec::new(),
        }
    }
    pub fn clear(&mut self) {
        self.type_vars.clear();
        self.region_vars.clear();
    }
    pub fn fresh_region(&mut self, loc: SrcLoc) -> usize {
        let next_var = self.region_vars.len();
        self.region_vars.push(RegionVarInfo { region: None, loc });
        next_var
    }
    pub fn fresh_ty(&mut self, loc: SrcLoc) -> usize {
        let next_var = self.type_vars.len();
        self.type_vars.push(TypeVarInfo { ty: None, loc });
        next_var
    }
    pub fn unsolved_locs(&self) -> Vec<SrcLoc> {
        self.type_vars
            .iter()
            .filter_map(|var| var.ty.is_none().then_some(var.loc.clone()))
            .chain(
                self.region_vars
                    .iter()
                    .filter_map(|var| var.region.is_none().then_some(var.loc.clone())),
            )
            .collect()
    }
    pub fn simplify_region(&self, region: Region) -> Region {
        let Ok(region) = Simplify(self).map_region(region);
        region
    }
    pub fn simplify_type(&self, ty: Type) -> Type {
        let Ok(ty) = Simplify(self).map_type(ty);
        ty
    }
    pub fn unify_region(&mut self, region1: Region, region2: Region) -> Option<Region> {
        match (region1, region2) {
            (r @ Region::Unknown, Region::Unknown) | (r @ Region::Static, Region::Static) => {
                Some(r)
            }
            (Region::Local(name1, index1), Region::Local(name2, index2)) if index1 == index2 => {
                assert_eq!(name1, name2);
                Some(Region::Local(name1, index1))
            }
            (Region::Param(name1, index1), Region::Param(name2, index2)) if index1 == index2 => {
                assert_eq!(name1, name2);
                Some(Region::Param(name1, index1))
            }
            (Region::Infer(var), Region::Infer(other)) if var == other => Some(Region::Infer(var)),
            (Region::Infer(var), r) | (r, Region::Infer(var)) => match &mut self.region_vars[var] {
                RegionVarInfo {
                    region: Some(entry),
                    ..
                } => {
                    let entry = entry.clone();
                    let r = self.unify_region(entry, r);
                    self.region_vars[var].region.clone_from(&r);
                    r
                }
                RegionVarInfo { region: entry, .. } => {
                    *entry = Some(r.clone());
                    Some(r)
                }
            },
            _ => None,
        }
    }
    pub fn unify_ty(&mut self, ty1: Type, ty2: Type) -> Option<Type> {
        match (ty1, ty2) {
            (ty @ Type::Int, Type::Int)
            | (ty @ Type::Bool, Type::Bool)
            | (ty @ Type::Unit, Type::Unit)
            | (ty @ Type::Unknown, Type::Unknown)
            | (ty @ Type::String, Type::String)
            | (ty @ Type::Char, Type::Char) => Some(ty),
            (Type::Param(name1, index1), Type::Param(name2, index2)) if index1 == index2 => {
                assert_eq!(name1, name2);
                Some(Type::Param(name1, index1))
            }
            (Type::Box(ty1), Type::Box(ty2)) => {
                self.unify_ty(*ty1, *ty2).map(|ty| Type::Box(Box::new(ty)))
            }
            (Type::Option(ty1), Type::Option(ty2)) => self
                .unify_ty(*ty1, *ty2)
                .map(|ty| Type::Option(Box::new(ty))),
            (Type::List(ty1), Type::List(ty2)) => {
                self.unify_ty(*ty1, *ty2).map(|ty| Type::List(Box::new(ty)))
            }
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
            (Type::Imm(region1, ty1), Type::Imm(region2, ty2)) => self
                .unify_ty(*ty1, *ty2)
                .and_then(|ty| {
                    self.unify_region(region1, region2)
                        .map(|region| (ty, region))
                })
                .map(|(ty, region)| Type::Imm(region, Box::new(ty))),
            (Type::Mut(region1, ty1), Type::Mut(region2, ty2)) => self
                .unify_ty(*ty1, *ty2)
                .and_then(|ty| {
                    self.unify_region(region1, region2)
                        .map(|region| (ty, region))
                })
                .map(|(ty, region)| Type::Mut(region, Box::new(ty))),
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
            _ => None,
        }
    }
}

struct Simplify<'a>(&'a TypeInfer);
impl TypeMap for Simplify<'_> {
    type Error = std::convert::Infallible;
    fn map_region(&mut self, region: Region) -> Result<Region, Self::Error> {
        let Region::Infer(var) = region else {
            return self.super_map_region(region);
        };
        if let RegionVarInfo {
            region: Some(region),
            loc: _,
        } = &self.0.region_vars[var]
        {
            self.map_region(region.clone())
        } else {
            Ok(region)
        }
    }
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
