use crate::types::{GenericArgs, TypeKind};

pub trait Visit {
    fn super_visit_type(&mut self, ty: &TypeKind) {
        match ty {
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
                for param in function_type.params.iter() {
                    self.visit_type(param);
                }
                self.visit_type(&function_type.return_type);
            }
            TypeKind::Tuple(fields) => {
                for ty in fields {
                    self.visit_type(ty);
                }
            }
            TypeKind::Record(fields) => {
                for field in fields {
                    self.visit_type(&field.ty);
                }
            }
            TypeKind::Array(ty) | TypeKind::Box(ty) => self.visit_type(ty),
            TypeKind::Named(.., generic_args) => {
                self.visit_generic_args(generic_args);
            }
        }
    }
    fn visit_type(&mut self, ty: &TypeKind) {
        self.super_visit_type(ty);
    }
    fn visit_generic_args(&mut self, args: &GenericArgs) {
        for arg in args.iter() {
            self.visit_type(&arg.0);
        }
    }
}

pub trait VisitMut {
    fn super_visit_type(&mut self, ty: &mut TypeKind) {
        match ty {
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
                for param in function_type.params.iter_mut() {
                    self.visit_type(param);
                }
                self.visit_type(&mut function_type.return_type);
            }
            TypeKind::Tuple(fields) => {
                for ty in fields {
                    self.visit_type(ty);
                }
            }
            TypeKind::Record(fields) => {
                for field in fields {
                    self.visit_type(&mut field.ty);
                }
            }
            TypeKind::Array(ty) | TypeKind::Box(ty) => self.visit_type(ty),
            TypeKind::Named(.., generic_args) => {
                self.visit_generic_args(generic_args);
            }
        }
    }
    fn visit_type(&mut self, ty: &mut TypeKind) {
        self.super_visit_type(ty);
    }
    fn visit_generic_args(&mut self, args: &mut GenericArgs) {
        for arg in args.iter_mut() {
            self.visit_type(&mut arg.0);
        }
    }
}
