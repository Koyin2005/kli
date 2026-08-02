use crate::{
    CtxtRef,
    types::{GenericArg, GenericArgs, Type, TypeKind},
};

pub trait Visit<'ctxt> {
    fn super_visit_type(&mut self, ty: Type<'ctxt>) {
        match ty.kind() {
            TypeKind::Infer(_)
            | TypeKind::Unknown
            | TypeKind::Int(_)
            | TypeKind::Bool
            | TypeKind::Char
            | TypeKind::Byte
            | TypeKind::Never
            | TypeKind::String
            | TypeKind::Param(..) => (),
            TypeKind::Function(function_type) => {
                for &param in function_type.params.iter() {
                    self.visit_type(param);
                }
                self.visit_type(function_type.return_type);
            }
            TypeKind::Tuple(fields) => {
                for &ty in fields {
                    self.visit_type(ty);
                }
            }
            &(TypeKind::Array(ty) | TypeKind::Box(ty) | TypeKind::RawArray(ty)) => {
                self.visit_type(ty)
            }
            TypeKind::Named(.., generic_args) => {
                self.visit_generic_args(generic_args);
            }
        }
    }
    fn visit_type(&mut self, ty: Type<'ctxt>) {
        self.super_visit_type(ty);
    }
    fn visit_generic_args(&mut self, args: &GenericArgs<'ctxt>) {
        for arg in args.iter() {
            self.visit_type(arg.0);
        }
    }
}

pub trait VisitMut<'ctxt> {
    fn ctxt(&self) -> CtxtRef<'ctxt>;
    fn super_visit_type(&mut self, ty: Type<'ctxt>) -> Type<'ctxt> {
        match ty.kind() {
            TypeKind::Infer(_)
            | TypeKind::Unknown
            | TypeKind::Int(_)
            | TypeKind::Bool
            | TypeKind::Char
            | TypeKind::Byte
            | TypeKind::Never
            | TypeKind::String
            | TypeKind::Param(..) => ty,
            TypeKind::Function(function_type) => {
                let params = function_type
                    .params
                    .iter()
                    .map(|&param| self.visit_type(param))
                    .collect();
                let return_type = self.visit_type(function_type.return_type);
                Type::function_type(self.ctxt(), params, return_type)
            }
            TypeKind::Tuple(fields) => Type::tuple_from_iter(
                self.ctxt(),
                fields.iter().map(|&field| self.visit_type(field)),
            ),
            &TypeKind::Array(ty) => Type::new_array(self.ctxt(), self.visit_type(ty)),
            &TypeKind::Box(ty) => Type::new_box(self.ctxt(), self.visit_type(ty)),
            &TypeKind::RawArray(ty) => Type::new_raw_array(self.ctxt(), self.visit_type(ty)),
            &TypeKind::Named(id, name, ref generic_args) => Type::named(
                self.ctxt(),
                id,
                name,
                self.visit_generic_args(generic_args.clone()),
            ),
        }
    }
    fn visit_type(&mut self, ty: Type<'ctxt>) -> Type<'ctxt> {
        self.super_visit_type(ty)
    }
    fn visit_generic_args(&mut self, args: GenericArgs<'ctxt>) -> GenericArgs<'ctxt> {
        args.into_iter()
            .map(|arg| GenericArg(self.visit_type(arg.0)))
            .collect()
    }
}
