use crate::collect::{CtxtRef, Generics};
use crate::def_ids::DefId;
use crate::resolved_ast::{self as res, TypeName};
use crate::src_loc::SrcLoc;
use crate::typecheck::infer::TypeInfer;
use crate::types::{GenericArg, GenericArgs, GenericKind, IntegerSize, Type};
use std::cell::RefCell;
pub struct Lower<'a, 'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    _id: DefId,
    infer: Option<&'a RefCell<TypeInfer<'ctxt>>>,
}
impl<'a, 'ctxt> Lower<'a, 'ctxt> {
    pub fn new(
        ctxt: CtxtRef<'ctxt>,
        id: DefId,
        infer: Option<&'a RefCell<TypeInfer<'ctxt>>>,
    ) -> Self {
        Self {
            ctxt,
            _id: id,
            infer,
        }
    }
    pub fn lower_types(
        &self,
        tys: &mut dyn Iterator<Item = &res::Type>,
    ) -> impl Iterator<Item = Type<'ctxt>> {
        tys.map(|ty| self.lower_type(ty))
    }
    fn lower_generic_args_with(
        &self,
        generics: Generics,
        count: usize,
        loc: SrcLoc,
        args: &res::GenericArgs,
    ) -> GenericArgs<'ctxt> {
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
                        GenericKind::Type => GenericArg::from_type(Type::UNKNOWN),
                    },
                };
                args.push(arg);
            }
            GenericArgs::from_vec(args)
        } else if let Some(infer) = self.infer {
            generics.instantiate(self.ctxt, &mut infer.borrow_mut(), loc)
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
    ) -> GenericArgs<'ctxt> {
        let generics = self.ctxt.generics(id);
        let count = generics.own_count();
        self.lower_generic_args_with(generics, count, loc, args)
    }
    pub fn lower_type_name(
        &self,
        loc: SrcLoc,
        name: TypeName,
        args: &res::GenericArgs,
    ) -> Type<'ctxt> {
        match name {
            TypeName::Param(name, param) => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::param(self.ctxt, name, param)
            }
            TypeName::UserDefined(id) => {
                let args = self.lower_generic_args(id, loc, args);
                Type::named(self.ctxt, id, self.ctxt.expect_ident(id).symbol, args)
            }
            TypeName::Int(size) => {
                let size = match size {
                    res::IntegerSize::Int64 => IntegerSize::Int64,
                    res::IntegerSize::Int32 => IntegerSize::Int32,
                    res::IntegerSize::Int8 => IntegerSize::Int8,
                };
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::new_int(self.ctxt, size)
            }
            TypeName::UInt(size) => {
                let size = match size {
                    res::IntegerSize::Int64 => IntegerSize::Int64,
                    res::IntegerSize::Int32 => IntegerSize::Int32,
                    res::IntegerSize::Int8 => IntegerSize::Int8,
                };
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::new_uint(self.ctxt, size)
            }
            TypeName::Bool => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::new_bool(self.ctxt)
            }
            TypeName::Char => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::new_char(self.ctxt)
            }
            TypeName::Never => {
                let _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::new_never(self.ctxt)
            }
            TypeName::Array => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    Type::new_unknown(self.ctxt)
                };
                Type::new_array(self.ctxt, ty)
            }
            TypeName::String => {
                _ = self.lower_generic_args_with(Generics::default(), 0, loc, args);
                Type::new_string(self.ctxt)
            }
            TypeName::Box => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    Type::new_unknown(self.ctxt)
                };
                Type::new_box(self.ctxt, ty)
            }
            TypeName::RawArray => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    Type::new_unknown(self.ctxt)
                };
                Type::new_raw_array(self.ctxt, ty)
            }
            TypeName::Uninit => {
                let args = self.lower_generic_args_with(Generics::default(), 1, loc, args);
                let ty = if let Ok([GenericArg(ty)]) = <[_; _]>::try_from(args) {
                    ty
                } else {
                    Type::new_unknown(self.ctxt)
                };
                Type::new_uninit(self.ctxt, ty)
            }
        }
    }
    pub fn lower_type(&self, ty: &res::Type) -> Type<'ctxt> {
        match &ty.kind {
            res::TypeKind::Tuple(fields) => {
                Type::tuple_from_iter(self.ctxt, fields.iter().map(|field| self.lower_type(field)))
            }
            res::TypeKind::Record(_) => {
                todo!("remove me")
            }
            res::TypeKind::Unknown => Type::UNKNOWN,
            &res::TypeKind::Named(name, ref args) => self.lower_type_name(ty.loc, name, args),
            res::TypeKind::Function(function_type) => {
                let res::FunctionType {
                    params,
                    return_type,
                } = function_type.as_ref();
                let params = self.lower_types(&mut params.iter()).collect();
                let return_type = self.lower_type(return_type);
                Type::function_type(self.ctxt, params, return_type)
            }
        }
    }
}
