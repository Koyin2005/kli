use crate::{
    ast::{Mutable, Pattern, PatternKind},
    typecheck::types::{Region, Type},
};

use super::root::TypeCheck;

impl TypeCheck {
    pub fn check_pattern(
        &mut self,
        pattern: &Pattern,
        expected_type: Type,
        region: Option<Region>,
    ) -> Type {
        let expected_type = self.simplify(expected_type);
        match &pattern.kind {
            PatternKind::None => {
                if let Type::Option(ty) = expected_type {
                    Type::Option(ty)
                } else if let Type::Option(ty) = expected_type.clone().strip_mut_quals() {
                    Type::Option(ty)
                } else {
                    self.diag.borrow_mut().report(
                        format!("Expected an option type but got '{}'", expected_type),
                        pattern.line,
                    );
                    self.unify(
                        expected_type,
                        Type::Option(Box::new(Type::Unknown)),
                        pattern.line,
                    )
                }
            }
            PatternKind::Some(pattern) => {
                if let Type::Option(ty) = expected_type {
                    Type::Option(Box::new(self.check_pattern(pattern, *ty, region)))
                } else if let Type::Option(ty) = expected_type.clone().strip_mut_quals() {
                    Type::Option(Box::new(self.check_pattern(pattern, *ty, region)))
                } else {
                    self.diag.borrow_mut().report(
                        format!("Expected an option type but got '{}'", expected_type),
                        pattern.line,
                    );
                    let ty = self.check_pattern(pattern, Type::Unknown, region);
                    self.unify(expected_type, Type::Option(Box::new(ty)), pattern.line)
                }
            }
            PatternKind::Binding(mutable, name, borrow_region) => {
                let borrow_region = borrow_region
                    .as_ref()
                    .map(|region| self.lower_region(region));
                match (borrow_region, region) {
                    (None, None) => {
                        self.declare_var(*mutable, &name.content, expected_type.clone());
                        expected_type
                    }
                    (None, Some(_)) => {
                        self.declare_var(*mutable, &name.content, expected_type.clone());
                        expected_type
                    }
                    (Some(region), None) => {
                        self.diag
                            .borrow_mut()
                            .report(format!("Cant borrow without region"), pattern.line);
                        let ty = match *mutable {
                            Mutable::Immutable => {
                                Type::Imm(region, Box::new(expected_type.clone()))
                            }
                            Mutable::Mutable => Type::Mut(region, Box::new(expected_type.clone())),
                        };
                        self.declare_var(*mutable, &name.content, ty.clone());
                        expected_type
                    }
                    (Some(borrow_region), Some(expected)) => {
                        if borrow_region != expected {
                            self.diag.borrow_mut().report(
                                format!("Expected '{expected}' but got '{borrow_region}'"),
                                pattern.line,
                            );
                        }
                        let ty = match *mutable {
                            Mutable::Immutable => {
                                Type::Imm(borrow_region, Box::new(expected_type.clone()))
                            }
                            Mutable::Mutable => {
                                Type::Mut(borrow_region, Box::new(expected_type.clone()))
                            }
                        };
                        self.declare_var(*mutable, &name.content, ty.clone());
                        expected_type
                    }
                }
            }
        }
    }
}
