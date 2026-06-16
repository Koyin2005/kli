use std::collections::HashMap;

use crate::{
    ast::{BinaryOp, IsResource},
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, Constant, ConstantValue, Local, Operand, OverflowOp,
        Place, Rvalue, Stmt, SwitchTarget, SwitchTargets, build::Builder,
    },
    typed_ast::{self, Expr, ExprKind, FieldId, IteratorType, Pattern},
    types::{FunctionType, Type},
};

impl Builder<'_> {
    fn as_constant(&mut self, expr: &Expr) -> Option<Constant> {
        match expr.kind {
            ExprKind::Bool(value) => Some(Constant::bool(value)),
            ExprKind::Int(value) => Some(Constant::int(value)),
            ExprKind::Unit => Some(Constant::unit()),
            ExprKind::Function(_, id, ref generic_args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty,
                    value: ConstantValue::Function(id, generic_args.clone()),
                })
            }
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
    fn place(&mut self, expr: &Expr) -> Place {
        if let Some(place) = self.as_place(expr) {
            place
        } else {
            Place::local(self.expr_into_temp(expr))
        }
    }
    fn operand_as_place(&mut self, ty: Type, operand: Operand) -> Place{
        match operand{
            Operand::Load(place) => place,
            Operand::Constant(_) => Place::local(self.assign_to_temp(ty, Rvalue::Use(operand)))
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
            typed_ast::PlaceKind::Deref(value) => self.place(value).with_deref(),
        }
    }
    fn expr_into_temp(&mut self, expr: &Expr) -> Local {
        let temp = self.new_temp(expr.ty.clone());
        self.expr_into_dest(Place::local(temp), expr);
        temp
    }
    fn for_loop(
        &mut self,
        pattern: &Pattern,
        iterator: &Expr,
        iterator_type: &IteratorType,
        body: &Expr,
    ) {
        match iterator_type {
            IteratorType::ArrayListRef(..) => {
                /*
                   for i in &l{
                       stuff
                   }

                   bb_header
                    iter = &l
                    i = 0
                    goto bb_cond
                   bb_cond
                    in_bounds = i < iter^.len
                    switch in_bounds 0 -> bb_end, otherwise -> bb_body
                   bb_body
                    ....
                    i = i + 1;
                    goto bb_cond
                   bb_end
                */
                let place = self.place(iterator);
                let current_index = self
                    .assign_to_temp(Type::Int, Rvalue::Use(Operand::Constant(Constant::int(0))));
                self.goto_to_new_block();

                //Condition
                let len = place.clone().with_deref().with_len();
                let in_bounds = self.assign_to_temp(
                    Type::Bool,
                    Rvalue::Binary(
                        mir::BinaryOp::Lesser,
                        Box::new((
                            Operand::Load(Place::local(current_index)),
                            Operand::Load(len),
                        )),
                    ),
                );
                let cond_block = self.current_block;

                //Body
                let loop_body_start_block = self.new_block();
                self.switch_to_block(loop_body_start_block);
                let current_element = place.with_deref().with_index(current_index);
                self.assign_place_to_pattern(pattern, current_element);
                self.expr_stmt(body);
                self.assign(
                    Place::local(current_index),
                    Rvalue::Binary(
                        mir::BinaryOp::Unchecked(OverflowOp::Add),
                        Box::new((
                            Operand::Load(Place::local(current_index)),
                            Operand::Constant(Constant::int(1)),
                        )),
                    ),
                );
                self.finish_block_with_goto(cond_block);
                self.switch_to_new_block();
                let end_block = self.current_block;
                self.switch_to_block(cond_block);
                self.finish_block_with_switch(
                    Operand::Load(Place::local(in_bounds)),
                    SwitchTargets {
                        targets: vec![SwitchTarget {
                            value: 0,
                            target: end_block,
                        }],
                        otherwise: loop_body_start_block,
                    },
                );
                self.switch_to_block(end_block);
            }
            IteratorType::StringIter(region, mutable) => {
                todo!("Char iterator")
            }
        }
    }
    fn panic(&mut self) {
        let block = self.new_block();
        self.finish_block(mir::Terminator::Panic);
        self.switch_to_block(block);
    }
    fn expr_stmt(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Err => (),
            ExprKind::Assign(place, expr) => {
                let place = self.lower_place(place);
                let value = self.build_rvalue(expr);
                self.assign(place, value);
            }
            ExprKind::Panic => {
                self.panic();
            }
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_stmt(&block_body.expr);
            }
            ExprKind::Print(expr) => {
                let stmt = Stmt::Print(expr.as_ref().map(|expr| self.operand(&**expr)));
                self.push_stmt(stmt);
            }
            ExprKind::For {
                pattern,
                iterator,
                body,
                iterator_type,
            } => {
                self.for_loop(pattern, iterator, iterator_type, body);
            }
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
            | ExprKind::Binary(..)
            | ExprKind::Function(..)
            | ExprKind::None
            | ExprKind::Some(..)
            | ExprKind::List(..)
            | ExprKind::Builtin(..)
            | ExprKind::Lambda(..) => {
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
    fn assign_place_to_pattern(&mut self, pattern: &Pattern, place: Place) {
        match &pattern.kind {
            typed_ast::PatternKind::Binding(borrowed, _, var, ty) => {
                let var_place = Place::local(self.new_var(var.clone(), (**ty).clone()));
                if let Some(borrowed) = borrowed {
                    self.assign(var_place, Rvalue::Ref(*borrowed, place));
                    return;
                }
                self.assign(var_place, Rvalue::Use(Operand::Load(place)));
            }
            typed_ast::PatternKind::Ref(pattern) => {
                self.assign_place_to_pattern(pattern, place.with_deref());
            }
            typed_ast::PatternKind::Bool(_)
            | typed_ast::PatternKind::Int(_)
            | typed_ast::PatternKind::None => (),
            
            _ => todo!("HANDL ASSIGN TO {:?}", pattern),
        }
    }
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
            ExprKind::Panic => {
                self.panic();
            }
            ExprKind::Record(_)
            | ExprKind::Function(..)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Unit
            | ExprKind::Load(_)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
            | ExprKind::List(..)
            | ExprKind::Print(_)
            | ExprKind::For { .. }
            | ExprKind::Assign(..)
            | ExprKind::Borrow { .. }
            | ExprKind::Some(_)
            | ExprKind::None
            | ExprKind::String(_)
            | ExprKind::Lambda(_)
            | ExprKind::Case(..) 
            | ExprKind::Builtin(..)=> {
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
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::Load(_)
            | ExprKind::Function(..) => {
                let operand = self
                    .as_operand(expr)
                    .unwrap_or_else(|| unreachable!("Should be an operand '{:?}' ", expr));
                Rvalue::Use(operand)
            }
            ExprKind::Record(fields) => {
                let mut field_map = fields
                    .iter()
                    .map(|field| {
                        (
                            field.index,
                            (field.name.clone(), self.operand(&field.value)),
                        )
                    })
                    .collect::<HashMap<_, _>>();
                let mut field_names = IndexVec::new();
                let fields = (0..fields.len())
                    .map(|field| {
                        let (name, operand) = field_map.remove(&FieldId::new(field)).unwrap();
                        field_names.push(name.content);
                        operand
                    })
                    .collect::<IndexVec<FieldId, _>>();
                Rvalue::Aggregate(AggregateKind::Record { field_names }, fields)
            }
            ExprKind::String(_) => todo!(),
            ExprKind::None => todo!(),
            ExprKind::Some(expr) => todo!(),
            ExprKind::Builtin(builtin, generic_args) => todo!(),
            ExprKind::List(exprs) => {
                let ty = if let Type::List(ty) = &expr.ty {
                    (**ty).clone()
                } else {
                    unreachable!("Should be an array")
                };
                let len_constant =
                    Operand::Constant(Constant::int(exprs.len().try_into().unwrap()));
                let new_array = self.assign_to_temp(
                    expr.ty.clone(),
                    Rvalue::AllocateArray(ty, len_constant.clone()),
                );
                for (i, expr) in exprs.iter().enumerate() {
                    let index: u32 = i.try_into().unwrap();
                    self.expr_into_dest(Place::local(new_array).with_constant_index(index), expr);
                }
                self.assign(
                    Place::local(new_array).with_len(),
                    Rvalue::Use(len_constant),
                );
                Rvalue::Use(Operand::Load(Place::local(new_array)))
            }
            ExprKind::Call(callee, args) => match &callee.ty {
                Type::Function(function_ty) => {
                    let FunctionType { resource, .. } = function_ty;
                    let callee_value = self.operand(callee);
                    let arg_values = args.iter().map(|arg| self.operand(arg)).collect::<Vec<_>>();
                    match resource {
                        IsResource::Data => {
                            Rvalue::Call(callee_value, arg_values)
                        }
                        IsResource::Resource => {
                            let closure_place = self.operand_as_place(callee.ty.clone(), callee_value);
                            let env = closure_place.clone().with_field(FieldId::new(0));
                            let code = closure_place.clone().with_field(FieldId::new(1));
                            let mut arg_values = arg_values;
                            arg_values.insert(0, Operand::Load(env));
                            Rvalue::Call(Operand::Load(code), arg_values)
                        }
                    }
                }
                _ => unreachable!("Can't call non function at {:?}", expr.loc),
            },
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
            ExprKind::Block(..) | ExprKind::Panic => {
                let temp = self.expr_into_temp(expr);
                Rvalue::Use(Operand::Load(Place::local(temp)))
            }
            ExprKind::For { .. } | ExprKind::Print(_) | ExprKind::Assign(..) => {
                self.expr_stmt(expr);
                Rvalue::Use(Operand::Constant(Constant::unit()))
            }
            ExprKind::Borrow {
                mutable,
                place,
                region: _,
            } => Rvalue::Ref(*mutable, self.lower_place(place)),
            ExprKind::Case(expr, case_arms) => todo!("case"),
            ExprKind::Lambda(lambda) => todo!("lambda"),
        }
    }
}
