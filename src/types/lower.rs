use std::cell::RefCell;
use std::collections::HashSet;

use crate::collect::{CtxtRef, Generics};
use crate::def_ids::DefId;
use crate::resolved_ast::{self as res, TypeName};
use crate::src_loc::SrcLoc;
use crate::typecheck::infer::TypeInfer;
use crate::types::{
    FieldName, FunctionType, GenericArg, GenericArgs, GenericKind, IntegerKind, RecordField, TypeKind,
};
pub struct Lower<'a,'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    _id: DefId,
    infer: Option<&'a RefCell<TypeInfer>>,
}
impl<'a,'ctxt> Lower<'a,'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>, id: DefId, infer: Option<&'a RefCell<TypeInfer>>) -> Self {
        Self {
            ctxt,
            _id: id,
            infer,
        }
    }
    pub fn lower_types(
        &self,
        tys: &mut dyn Iterator<Item = &res::Type>,
    ) -> impl Iterator<Item = TypeKind> {
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
            let mut args = Vec::new();
            loop {
                let arg = match (args_iter.next(), kind_iter.next()) {
                    (None, None) => break,
                    (Some(arg), Some(kind)) => match (arg, kind) {
                        (res::GenericArg::Type(ty), GenericKind::Type) => {
                            GenericArg::from_type(self.lower_type(ty))
                        }
                    },
                    (Some(arg), None) => match arg {
                        res::GenericArg::Type(ty) => GenericArg::from_type(self.lower_type(ty)),
                    },
                    (None, Some(kind)) => match kind {
                        GenericKind::Type => GenericArg::from_type(TypeKind::Unknown),
                    },
                };
                args.push(arg);
            }
            GenericArgs::from_vec(args)
        } else if let Some(infer) = self.infer {
            generics.instantiate(&mut infer.borrow_mut(), loc)
        } else if arg_count > 0 {
            self.ctxt.diag().add_diagnostic(
                format!("Expected '{}' generic args but got none", arg_count,),
                loc,
            );
            generics.instantiate_unknown()
        } else {
            GenericArgs::new()
        }
    }
    pub fn lower_generic_args(
        &self,
        id: DefId,
        loc: SrcLoc,
        args: &res::GenericArgs,
    ) -> GenericArgs {
        let generics = self.ctxt.generics(id);
        let count = generics.own_count();
        self.lower_generic_args_with(generics, count, loc, args)
    }
    pub fn lower_type_name(&self, loc: SrcLoc, name: TypeName, args: &res::GenericArgs) -> TypeKind {
        match name {
            TypeName::Param(name, param) => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Param(name, param)
            }
            TypeName::UserDefined(id) => {
                let args = self.lower_generic_args(id, loc, args);
                TypeKind::Named(id, self.ctxt.expect_ident(id).symbol, args)
            }
            TypeName::Byte => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Byte
            }
            TypeName::Bool => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Bool
            }
            TypeName::Int => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Int(IntegerKind::Signed)
            }
            TypeName::Uint => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Int(IntegerKind::Unsigned)
            }
            TypeName::Char => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Char
            }
            TypeName::Never => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::Never
            }
            TypeName::Array => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    TypeKind::Unknown
                };
                TypeKind::Array(Box::new(ty))
            }
            TypeName::String => {
                _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                TypeKind::String
            }
            TypeName::Box => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    TypeKind::Unknown
                };
                TypeKind::Box(Box::new(ty))
            }
        }
    }
    pub fn lower_type(&self, ty: &res::Type) -> TypeKind {
        match &ty.kind {
            res::TypeKind::Tuple(fields) => {
                TypeKind::tuple(fields.iter().map(|field| self.lower_type(field)))
            }
            res::TypeKind::Record(fields) => TypeKind::Record({
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
            res::TypeKind::Unknown => TypeKind::Unknown,
            &res::TypeKind::Named(name, ref args) => self.lower_type_name(ty.loc, name, args),
            res::TypeKind::Function(function_type) => {
                let res::FunctionType {
                    params,
                    return_type,
                } = function_type.as_ref();
                let params = self.lower_types(&mut params.iter()).collect();
                let return_type = self.lower_type(return_type);
                TypeKind::Function(FunctionType {
                    params,
                    return_type: Box::new(return_type),
                })
            }
        }
    }
}
