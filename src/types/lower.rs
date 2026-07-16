use std::cell::RefCell;
use std::collections::HashSet;

use crate::collect::{CtxtRef, Generics};
use crate::def_ids::DefId;
use crate::lang_items::LangItem;
use crate::resolved_ast::{self as res, TypeName};
use crate::src_loc::SrcLoc;
use crate::typecheck::infer::TypeInfer;
use crate::types::{
    FieldName, FunctionType, GenericArg, GenericArgs, GenericKind, IntegerKind, RecordField,
    Region, Type,
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
            let mut args_iter = args.iter();
            let mut kind_iter = generics.kinds();
            let mut args = GenericArgs::new();
            loop {
                let arg = match (args_iter.next(), kind_iter.next()) {
                    (None, None) => break,
                    (Some(arg), Some(kind)) => match (arg, kind) {
                        (res::GenericArg::Type(ty), GenericKind::Type) => {
                            GenericArg::Type(self.lower_type(ty))
                        }
                        (res::GenericArg::Region(region), GenericKind::Region) => {
                            GenericArg::Region(self.lower_region(region))
                        }
                        (_, kind @ (GenericKind::Region | GenericKind::Type)) => {
                            self.ctxt
                                .diag()
                                .add_diagnostic("Generic kind mismatch".to_string(), arg.loc());
                            match kind {
                                GenericKind::Region => GenericArg::Region(Region::Unknown),
                                GenericKind::Type => GenericArg::Type(Type::Unknown),
                            }
                        }
                    },
                    (Some(arg), None) => match arg {
                        res::GenericArg::Region(region) => {
                            GenericArg::Region(self.lower_region(region))
                        }
                        res::GenericArg::Type(ty) => GenericArg::Type(self.lower_type(ty)),
                    },
                    (None, Some(kind)) => match kind {
                        GenericKind::Region => GenericArg::Region(Region::Unknown),
                        GenericKind::Type => GenericArg::Type(Type::Unknown),
                    },
                };
                args.push(arg);
            }
            args
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
                Type::Int(IntegerKind::Signed)
            }
            TypeName::Uint => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Int(IntegerKind::Unsigned)
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
            TypeName::Never => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::Never
            }
            TypeName::Box | TypeName::ArrayList | TypeName::String => {
                let id = self.ctxt.lang_items().expect(match name {
                    TypeName::String => LangItem::String,
                    TypeName::ArrayList => LangItem::ArrayList,
                    TypeName::Box => LangItem::Box,
                    _ => unreachable!(),
                });
                let args = self.lower_generic_args(id, loc, args);
                Type::Named(id, self.ctxt.expect_ident(id).symbol, args)
            }
            TypeName::Pair => {
                let args = self.lower_generic_args_with(Generics::default(), 2, loc, args);
                let into_type_arg = move |arg: GenericArg| -> Type {
                    match arg {
                        GenericArg::Type(ty) => ty,
                        GenericArg::Region(region) => {
                            self.ctxt.diag().add_diagnostic(
                                format!("Expected a 'type' but got region '{region}'"),
                                loc,
                            );
                            Type::Unknown
                        }
                    }
                };
                let mut args = args.into_iter();
                let first = args.next().map_or(Type::Unknown, into_type_arg);
                let second = args.next().map_or(Type::Unknown, into_type_arg);
                Type::pair(first, second)
            }
        }
    }
    pub fn lower_type(&self, ty: &res::Type) -> Type {
        match &ty.kind {
            res::TypeKind::Tuple(fields) => {
                Type::tuple(fields.iter().map(|field| self.lower_type(field)))
            }
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
