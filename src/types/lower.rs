use std::cell::RefCell;
use std::collections::HashSet;

use crate::collect::{CtxtRef, Generics};
use crate::def_ids::DefId;
use crate::lang_items::LangItem;
use crate::resolved_ast::{self as res, TypeName};
use crate::src_loc::SrcLoc;
use crate::typecheck::infer::TypeInfer;
use crate::types::{
    FieldName, FunctionType, GenericArg, GenericArgs, GenericKind, RecordField, Region, Type,
};
pub struct Lower<'a> {
    ctxt: CtxtRef<'a>,
    id: DefId,
    infer: Option<&'a RefCell<TypeInfer>>,
}
impl<'a> Lower<'a> {
    pub fn new(ctxt: CtxtRef<'a>, id: DefId, infer: Option<&'a RefCell<TypeInfer>>) -> Self {
        Self { ctxt, id, infer }
    }

    pub fn lower_region(&self, region: &res::Region) -> Region {
        match &region.kind {
            &res::RegionKind::Param(name, param) => {
                if let GenericKind::Region = self.ctxt.generics(self.id).kind(param) {
                    Region::Param(name, param)
                } else {
                    self.ctxt
                        .diag()
                        .add_diagnostic(format!("Cannot use '{}' as region", name), region.loc);
                    Region::Unknown
                }
            }
            &res::RegionKind::Local(name, id) => Region::Local(name, id),
            res::RegionKind::Static => Region::Static,
            res::RegionKind::Unknown => Region::Unknown,
        }
    }
    pub fn lower_types(
        &self,
        tys: &mut dyn Iterator<Item = &res::Type>,
    ) -> impl Iterator<Item = Type> {
        tys.map(|ty| self.lower_type(ty))
    }
    fn lower_generic_args_with(
        &self,
        generics: Generics,
        count: usize,
        loc: SrcLoc,
        args: &res::GenericArgs,
    ) -> GenericArgs {
        let arg_count = count;
        let loc = args.loc.unwrap_or(loc);
        if let Some(args) = args.args() {
            if arg_count != args.len() {
                self.ctxt.diag().add_diagnostic(
                    format!(
                        "Expected '{}' generic args but got '{}'",
                        arg_count,
                        args.len()
                    ),
                    loc,
                );
            }
            let remaining = args.len().abs_diff(arg_count);
            args.iter()
                .map(|arg| match arg {
                    res::GenericArg::Region(region) => {
                        GenericArg::Region(self.lower_region(region))
                    }
                    res::GenericArg::Type(ty) => GenericArg::Type(self.lower_type(ty)),
                })
                .chain(std::iter::repeat_n(
                    GenericArg::Type(Type::Unknown),
                    remaining,
                ))
                .collect()
        } else if let Some(infer) = self.infer {
            generics.instantiate(&mut infer.borrow_mut(), loc)
        } else if arg_count > 0 {
            self.ctxt.diag().add_diagnostic(
                format!("Expected '{}' generic args but got none", arg_count,),
                loc,
            );
            generics.instantiate_unknown()
        } else {
            Vec::new()
        }
    }
    pub fn lower_generic_args(
        &self,
        id: DefId,
        loc: SrcLoc,
        args: &res::GenericArgs,
    ) -> GenericArgs {
        let generics = self.ctxt.generics(id);
        let count = generics.count();
        self.lower_generic_args_with(generics, count, loc, args)
    }
    pub fn lower_type_name(&self, loc: SrcLoc, name: TypeName, args: &res::GenericArgs) -> Type {
        match name {
            TypeName::Param(name, param) => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                if let GenericKind::Type = self.ctxt.generics(self.id).kind(param) {
                    Type::Param(name, param)
                } else {
                    self.ctxt
                        .diag()
                        .add_diagnostic(format!("Cannot use '{}' as a type", name), loc);
                    Type::Unknown
                }
            }
            TypeName::UserDefined(id) => {
                let args = self.lower_generic_args(id, loc, args);
                Type::Named(id, self.ctxt.expect_ident(id).symbol, args)
            }
            TypeName::Byte => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Byte
            }
            TypeName::Bool => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Bool
            }
            TypeName::Unit => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Unit
            }
            TypeName::Int => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Int
            }
            TypeName::String => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::String
            }
            TypeName::Char => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Char
            }
            TypeName::Ptr => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg::Type(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    self.ctxt
                        .diag()
                        .add_diagnostic("Expected a 'type' generic arg for 'ptr'".to_string(), loc);
                    Type::Unknown
                };
                Type::pointer(ty)
            }
            TypeName::Box => {
                let id = self.ctxt.lang_items().expect(LangItem::Box);
                let args = self.lower_generic_args(id, loc, args);
                Type::Named(id, self.ctxt.expect_ident(id).symbol, args)
            }
            TypeName::ArrayList => {
                let id = self.ctxt.lang_items().expect(LangItem::ArrayList);
                let args = self.lower_generic_args(id, loc, args);
                Type::Named(id, self.ctxt.expect_ident(id).symbol, args)
            }
        }
    }
    pub fn lower_type(&self, ty: &res::Type) -> Type {
        match &ty.kind {
            res::TypeKind::Ptr(pointee) => Type::pointer(self.lower_type(pointee)),
            res::TypeKind::Record(fields) => Type::Record({
                let mut seen_fields = HashSet::new();
                fields
                    .iter()
                    .filter_map(|field| {
                        if !seen_fields.insert(field.name.symbol) {
                            self.ctxt.diag().add_diagnostic(
                                format!("Repeated field '{}'", field.name.symbol),
                                field.name.loc,
                            );
                            return None;
                        }
                        Some(RecordField {
                            name: FieldName::Named(field.name.symbol),
                            ty: self.lower_type(&field.ty),
                        })
                    })
                    .collect()
            }),
            res::TypeKind::Unknown => Type::Unknown,
            &res::TypeKind::Named(name, ref args) => self.lower_type_name(ty.loc, name, args),
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
            res::TypeKind::Function(function_type) => {
                let res::FunctionType {
                    is_resource,
                    params,
                    return_type,
                } = function_type.as_ref();
                let params = self.lower_types(&mut params.iter()).collect();
                let return_type = self.lower_type(return_type);
                Type::Function(FunctionType {
                    resource: *is_resource,
                    params,
                    return_type: Box::new(return_type),
                })
            }
        }
    }
}
