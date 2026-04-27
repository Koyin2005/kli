use std::cell::RefCell;

use crate::diagnostics::DiagnosticReporter;
use crate::resolved_ast as res;
use crate::types::{FunctionType, GenericKind, Region, Type};
pub struct Lower<'a> {
    kinds: &'a [GenericKind],
    diag: &'a RefCell<DiagnosticReporter>,
}
impl<'a> Lower<'a> {
    pub fn new(kinds: &'a [GenericKind], diag: &'a RefCell<DiagnosticReporter>) -> Self {
        Self { kinds, diag }
    }

    pub(super) fn lower_region(&self, region: &res::Region) -> Region {
        match &region.kind {
            res::RegionKind::Param(name, param) => {
                if let GenericKind::Region = self.kinds[*param] {
                    Region::Param(name.clone(), *param)
                } else {
                    self.diag
                        .borrow_mut()
                        .report(format!("Cannot use '{}' as region", name), region.line);
                    Region::Unknown
                }
            }
            res::RegionKind::Local(name, id) => Region::Local(name.clone(), *id),
            res::RegionKind::Static => Region::Static,
            res::RegionKind::Unknown => Region::Unknown,
        }
    }
    pub(super) fn lower_types(&self, tys: &mut dyn Iterator<Item = &res::Type>) -> Vec<Type> {
        tys.map(|ty| self.lower_type(ty)).collect()
    }
    pub(super) fn lower_type(&self, ty: &res::Type) -> Type {
        match &ty.kind {
            res::TypeKind::Unknown => Type::Unknown,
            res::TypeKind::Bool => Type::Bool,
            res::TypeKind::Int => Type::Int,
            res::TypeKind::Unit => Type::Unit,
            res::TypeKind::String => Type::String,
            res::TypeKind::Option(ty) => Type::Option(Box::new(self.lower_type(ty))),
            res::TypeKind::Box(ty) => Type::Box(Box::new(self.lower_type(ty))),
            res::TypeKind::List(ty) => Type::List(Box::new(self.lower_type(ty))),
            res::TypeKind::Imm(region, ty) => {
                let region = self.lower_region(region);
                let ty = self.lower_type(ty);
                Type::Imm(region, Box::new(ty))
            }
            res::TypeKind::Mut(region, ty) => {
                let region = self.lower_region(region);
                let ty = self.lower_type(ty);
                Type::Mut(region, Box::new(ty))
            }
            res::TypeKind::Function(params, return_ty) => {
                let params = self.lower_types(&mut params.iter());
                let return_type = self.lower_type(return_ty);
                Type::Function(FunctionType {
                    params,
                    return_type: Box::new(return_type),
                })
            }
            &res::TypeKind::Param(ref name, param) => {
                if let GenericKind::Type = self.kinds[param] {
                    Type::Param(name.clone(), param)
                } else {
                    self.diag
                        .borrow_mut()
                        .report(format!("Cannot use '{}' as a type", name), ty.line);
                    Type::Unknown
                }
            }
        }
    }
}
