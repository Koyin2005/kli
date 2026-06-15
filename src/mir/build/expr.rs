use crate::{
    ast::BinaryOp,
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, BasicBlock, Constant, ConstantValue, Local, Operand, OverflowOp,
        Place, Rvalue, Stmt, build::Builder,
    },
    typed_ast::{self, Expr, ExprKind, FieldId, Pattern},
    types::Type,
};

impl Builder<'_> {
    fn as_constant(&mut self, expr: &Expr) -> Option<Constant> {
        match expr.kind {
            ExprKind::Bool(value) => Some(Constant::bool(value)),
            ExprKind::Int(value) => Some(Constant::int(value)),
            ExprKind::Unit => Some(Constant::unit()),
            _ => None,
        }
    }
    fn as_place(&mut self, expr: &Expr) -> Option<Place> {
        if let ExprKind::Load(place) = &expr.kind {
            Some(self.lower_place(place))
        } else {
            None
        }
    }
    fn as_operand(&mut self, expr: &Expr) -> Option<Operand> {
        if let Some(constant) = self.as_constant(expr) {
            Some(Operand::Constant(constant))
        } else if let Some(place) = self.as_place(expr) {
            Some(Operand::Load(place))
        } else {
            None
        }
    }
    fn operand(&mut self, expr: &Expr) -> Operand {
        if let Some(operand) = self.as_operand(expr) {
            operand
        } else {
            Operand::Load(Place::local(self.expr_into_temp(expr)))
        }
    }
    fn assign(&mut self, place: Place, value: Rvalue) {
        self.push_stmt(Stmt::Assign(place, value));
    }
    fn assign_to_temp(&mut self, ty: Type, value: Rvalue) -> Local {
        let temp = self.new_temp(ty);
        self.push_stmt(Stmt::Assign(Place::local(temp), value));
        temp
    }
    fn lower_place(&mut self, place: &typed_ast::Place) -> Place {
        match &place.kind {
            typed_ast::PlaceKind::Var(var) => {
                let local = self.body.local_for_var(var.1).unwrap();
                Place::local(local)
            }
            typed_ast::PlaceKind::Deref(_) => todo!("Derefs"),
        }
    }
    fn expr_into_temp(&mut self, expr: &Expr) -> Local {
        let temp = self.new_temp(expr.ty.clone());
        self.expr_into_dest(Place::local(temp), expr);
        temp
    }
    fn expr_stmt(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Err => (),
            ExprKind::Assign(place, expr) => {
                let place = self.lower_place(place);
                let value = self.build_rvalue(expr);
                self.assign(place, value);
            }
            ExprKind::Block(block_body, local_region_id) => todo!(),
            ExprKind::None => todo!(),
            ExprKind::Panic => todo!(),
            ExprKind::Some(expr) => todo!(),
            ExprKind::Builtin(builtin, generic_args) => todo!(),
            ExprKind::Function(_, function_id, generic_args) => todo!(),
            ExprKind::Print(expr) => todo!(),
            ExprKind::List(exprs) => todo!(),
            ExprKind::For { .. } => todo!(),
            ExprKind::Lambda(..) => todo!(),
            //Evaluate
            ExprKind::Record(..)
            | ExprKind::String(_)
            | ExprKind::Unit
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Borrow { .. }
            | ExprKind::Load(_)
            | ExprKind::Case(..)
            | ExprKind::Call(..)
            | ExprKind::Binary(..) => {
                self.expr_into_temp(expr);
            }
        }
    }
    fn assign_to_pattern(&mut self, pattern: &Pattern, value: &Expr) {
        match &pattern.kind {
            typed_ast::PatternKind::Binding(None, _, var, ty) => {
                let place = Place::local(self.new_var(var.clone(), (**ty).clone()));
                self.expr_into_dest(place, value);
            }
            _ => {
                let place = Place::local(self.expr_into_temp(value));
                self.assign_place_to_pattern(pattern, place);
            }
        }
    }
    fn assign_place_to_pattern(&mut self, pattern: &Pattern, place: Place) {}
    pub fn stmt(&mut self, stmt: &typed_ast::Stmt) {
        match &stmt.kind {
            typed_ast::StmtKind::Expr(expr) => {
                self.expr_stmt(expr);
            }
            typed_ast::StmtKind::Let(binding) => {
                self.assign_to_pattern(&binding.pattern, &binding.value);
            }
        }
    }
    pub fn expr_into_dest(&mut self, dest: Place, expr: &Expr) {
        match &expr.kind {
            ExprKind::Err => unreachable!("Cannot have err here"),
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_into_dest(dest, &block_body.expr);
            }
            ExprKind::String(_) => todo!(),
            ExprKind::None => todo!(),
            ExprKind::Panic => todo!(),
            ExprKind::Some(expr) => todo!(),
            ExprKind::Builtin(builtin, generic_args) => todo!(),
            ExprKind::Function(_, function_id, generic_args) => todo!(),
            ExprKind::Print(expr) => todo!(),
            ExprKind::For {
                pattern,
                iterator,
                body,
            } => todo!(),
            ExprKind::Borrow {
                mutable,
                place,
                region,
            } => todo!(),
            ExprKind::Case(expr, case_arms) => todo!("Case"),
            ExprKind::Assign(place, expr) => todo!(),
            ExprKind::Lambda(lambda) => todo!(),
            //Rvalue exprs
            ExprKind::Record(_)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Unit
            | ExprKind::Load(_)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
            | ExprKind::List(..) => {
                let rvalue = self.build_rvalue(expr);
                self.assign(dest, rvalue);
            }
        }
    }
    fn binary_op_rvalue(op: mir::BinaryOp, left: Operand, right: Operand) -> Rvalue {
        Rvalue::Binary(op, Box::new((left, right)))
    }
    pub fn build_rvalue(&mut self, expr: &Expr) -> Rvalue {
        match &expr.kind {
            ExprKind::Err => unreachable!("Cannot have err here"),
            ExprKind::Unit | ExprKind::Int(_) | ExprKind::Bool(_) | ExprKind::Load(_) => {
                let operand = self.as_operand(expr).unwrap();
                Rvalue::Use(operand)
            }
            ExprKind::Record(_) => {
                todo!("Record")
            }
            ExprKind::Block(block_body, local_region_id) => todo!(),
            ExprKind::String(_) => todo!(),
            ExprKind::None => todo!(),
            ExprKind::Panic => todo!(),
            ExprKind::Some(expr) => todo!(),
            ExprKind::Builtin(builtin, generic_args) => todo!(),
            ExprKind::Function(_, function_id, generic_args) => todo!(),
            ExprKind::Print(expr) => todo!(),
            ExprKind::List(exprs) => {
                // allocate space for n values
                todo!("List")
            },
            ExprKind::Call(expr, exprs) => todo!(),
            ExprKind::Binary(binary_op, left, right) => {
                let left_operand = self.operand(left);
                let right_operand = self.operand(right);
                let overflow_op = match binary_op {
                    BinaryOp::Add => OverflowOp::Add,
                    BinaryOp::Divide => {
                        //Division can fail in 2 ways
                        //Divide by zero
                        //Divide int min by -1
                        let is_zero = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::Equals,
                                right_operand.clone(),
                                Operand::Constant(Constant::int(0)),
                            ),
                        );
                        self.assert(
                            Operand::Load(Place::local(is_zero)),
                            mir::AssertKind::DivideByZero,
                        );
                        let is_left_min = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::Equals,
                                left_operand.clone(),
                                Operand::Constant(Constant::int(ConstantValue::MIN_INT)),
                            ),
                        );
                        let is_right_neg_1 = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::Equals,
                                left_operand.clone(),
                                Operand::Constant(Constant::int(-1)),
                            ),
                        );
                        let overflow = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::BitwiseAnd,
                                Operand::Load(Place::local(is_left_min)),
                                Operand::Load(Place::local(is_right_neg_1)),
                            ),
                        );
                        self.assert(
                            Operand::Load(Place::local(overflow)),
                            mir::AssertKind::DivideOverflow,
                        );
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::Divide,
                            left_operand,
                            right_operand,
                        );
                    }
                    BinaryOp::Subtract => OverflowOp::Subtract,
                    BinaryOp::Multiply => OverflowOp::Multiply,
                };
                let checked_result = self.assign_to_temp(
                    Type::record(vec![Type::Bool, Type::Int]),
                    Rvalue::Binary(
                        mir::BinaryOp::Overflow(overflow_op),
                        Box::new((left_operand, right_operand)),
                    ),
                );
                let overflow =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(0)));
                self.assert(overflow, mir::AssertKind::Overflow(overflow_op));
                let result =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(1)));
                Rvalue::Use(result)
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
            } => todo!(),
            ExprKind::Borrow {
                mutable,
                place,
                region,
            } => todo!(),
            ExprKind::Case(expr, case_arms) => todo!(),
            ExprKind::Assign(place, expr) => {
                todo!()
            }
            ExprKind::Lambda(lambda) => todo!(),
        }
    }
}
