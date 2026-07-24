use crate::types::{GenericArgs, Type};

pub trait Visit {
    fn super_visit_type(&mut self, ty: &Type) {
        match ty {
            Type::Infer(_)
            | Type::Unknown
            | Type::Int(_)
            | Type::Bool
            | Type::Char
            | Type::Byte
            | Type::Never
            | Type::Param(..) => (),
            Type::Function(function_type) => {
                for param in function_type.params.iter() {
                    self.visit_type(param);
                }
                self.visit_type(&function_type.return_type);
            }
            Type::Tuple(fields) => {
                for ty in fields {
                    self.visit_type(ty);
                }
            }
            Type::Record(fields) => {
                for field in fields {
                    self.visit_type(&field.ty);
                }
            }
            Type::Array(ty) => self.visit_type(ty),
            Type::Named(.., generic_args) => {
                self.visit_generic_args(generic_args);
            }
        }
    }
    fn visit_type(&mut self, ty: &Type) {
        self.super_visit_type(ty);
    }
    fn visit_generic_args(&mut self, args: &GenericArgs) {
        for arg in args.iter() {
            self.visit_type(&arg.0);
        }
    }
}

pub trait VisitMut {
    fn super_visit_type(&mut self, ty: &mut Type) {
        match ty {
            Type::Infer(_)
            | Type::Unknown
            | Type::Int(_)
            | Type::Bool
            | Type::Char
            | Type::Byte
            | Type::Never
            | Type::Param(..) => (),
            Type::Function(function_type) => {
                for param in function_type.params.iter_mut() {
                    self.visit_type(param);
                }
                self.visit_type(&mut function_type.return_type);
            }
            Type::Tuple(fields) => {
                for ty in fields {
                    self.visit_type(ty);
                }
            }
            Type::Record(fields) => {
                for field in fields {
                    self.visit_type(&mut field.ty);
                }
            }
            Type::Array(ty) => self.visit_type(ty),
            Type::Named(.., generic_args) => {
                self.visit_generic_args(generic_args);
            }
        }
    }
    fn visit_type(&mut self, ty: &mut Type) {
        self.super_visit_type(ty);
    }
    fn visit_generic_args(&mut self, args: &mut GenericArgs) {
        for arg in args.iter_mut() {
            self.visit_type(&mut arg.0);
        }
    }
}
