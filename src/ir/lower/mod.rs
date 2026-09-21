use std::collections::HashMap;

use crate::{
    CtxtRef, Symbol,
    ast::Mutable,
    builtins::Builtin,
    def_ids::DefId,
    ir::{self, BodyId, Local, print::Print},
    resolved_ast::{Var, VarId},
    typed_ast::{self, Expr, LogicalOp, Pattern, PatternKind, Place, Stmt, StmtKind},
    types::{Type, TypeKind},
};

struct LoweringCtxt {
    id_map: HashMap<DefId, BodyId>,
}
impl LoweringCtxt {
    fn expect_body_id(&self, id: DefId) -> BodyId {
        let Some(&id) = self.id_map.get(&id) else {
            panic!("should have an id for '{:?}'", id)
        };
        id
    }
}
pub(super) struct LowerFunction<'a, 'ctxt> {
    vars: HashMap<VarId, Local>,
    function: &'a typed_ast::Function<'ctxt>,
    ctxt: &'a LoweringCtxt,
    body: ir::Body,
    stmts: Vec<ir::Stmt>,
    loop_label: u32,
}

impl<'a, 'ctxt> LowerFunction<'a, 'ctxt> {
    fn new(
        lower_ctxt: &'a LoweringCtxt,
        ctxt: CtxtRef<'ctxt>,
        id: DefId,
        function: &'a typed_ast::Function<'ctxt>,
    ) -> Self {
        let param_count = function.params.len();
        Self {
            ctxt: lower_ctxt,
            loop_label: 0,
            vars: function
                .params
                .iter()
                .enumerate()
                .filter_map(|(i, param)| param.var.map(|var| (var, Local::new(i))))
                .collect(),
            function,
            body: ir::Body {
                name: ctxt.expect_ident(id).symbol.to_string(),
                generic_params: ctxt.generics(id).names().collect(),
                param_count: param_count as u32,
                return_ty: lower_type(function.return_type),
                locals: {
                    function
                        .params
                        .iter()
                        .map(|param| ir::LocalInfo {
                            ty: lower_type(param.ty),
                            is_mutable: true,
                            name: Some(param.name.symbol),
                        })
                        .collect()
                },
                body: Vec::new(),
            },
            stmts: Vec::new(),
        }
    }
    fn push_stmt(&mut self, stmt: ir::Stmt) {
        self.stmts.push(stmt);
    }
    fn stmts_for<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (Vec<ir::Stmt>, T) {
        let old_stmts = std::mem::take(&mut self.stmts);
        let result = f(self);
        let stmts = std::mem::replace(&mut self.stmts, old_stmts);
        (stmts, result)
    }
    fn fresh_local_for_var(&mut self, var: Var, mutable: bool, ty: ir::Type) -> Local {
        let local = self.body.locals.push(ir::LocalInfo {
            name: Some(var.0),
            is_mutable: mutable,
            ty,
        });
        self.vars.insert(var.1, local);
        local
    }
    fn fresh_temp(&mut self, ty: ir::Type) -> Local {
        self.body.locals.push(ir::LocalInfo {
            ty,
            name: None,
            is_mutable: false,
        })
    }
    fn lower_expr_to_place(&mut self, expr: &Expr<'ctxt>) -> ir::Place {
        if let Some(result) = self.lower_expr(expr) {
            if let ir::ExprKind::Load(place) = result.kind {
                place
            } else {
                let tmp = self.fresh_temp(lower_type(expr.ty));
                self.push_stmt(ir::Stmt::Assign(ir::Place::Local(tmp), result));
                ir::Place::Local(tmp)
            }
        } else {
            let tmp = self.fresh_temp(lower_type(expr.ty));
            ir::Place::Local(tmp)
        }
    }
    fn lower_stmt(&mut self, stmt: &Stmt<'ctxt>) {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                let ty = lower_type(expr.ty);
                let tmp = self.fresh_temp(ty);
                self.lower_expr_into(ir::Place::Local(tmp), expr);
            }
            StmtKind::Let(binding) => {
                self.assign_to_pattern(&binding.pattern, &binding.value);
            }
        }
    }
    fn assign_to_pattern(&mut self, pattern: &Pattern<'ctxt>, expr: &Expr<'ctxt>) {
        match pattern.kind {
            PatternKind::Binding(mutable, var, ty) => {
                let ty = lower_type(ty);
                let local = self.fresh_local_for_var(var, matches!(mutable, Mutable::Mutable), ty);
                self.lower_expr_into(ir::Place::Local(local), expr);
            }
            PatternKind::Unit | PatternKind::Bool(_) | PatternKind::Int(_) => (),
            _ => {
                let result = self.lower_expr_to_place(&expr);
                self.lower_place_to_pattern(result, pattern);
            }
        }
    }
    fn lower_place_to_pattern(&mut self, place: ir::Place, pattern: &Pattern<'ctxt>) {
        match &pattern.kind {
            PatternKind::Binding(mutable, var, ty) => {
                let ty = lower_type(*ty);
                let local = self.fresh_local_for_var(*var, matches!(mutable, Mutable::Mutable), ty);
                self.push_stmt(ir::Stmt::Assign(
                    ir::Place::Local(local),
                    ir::Expr::load(place),
                ));
            }
            PatternKind::Err => unreachable!(),
            PatternKind::Unit => todo!(),
            PatternKind::Int(_) => todo!(),
            PatternKind::Bool(_) => todo!(),
            PatternKind::Char(_) => todo!(),
            PatternKind::Case(..) => todo!(),
            PatternKind::Record(_) => todo!(),
        }
    }
    fn lower_exprs_const<const N: usize>(&mut self, exprs: &[Expr<'ctxt>]) -> [ir::Expr; N] {
        let Ok(exprs) = exprs
            .iter()
            .map(|expr| self.lower_expr(expr).expect("should produce a value"))
            .collect::<Vec<_>>()
            .try_into()
        else {
            panic!("should have N value producing expressions {}", N)
        };
        exprs
    }
    fn lower_place(&mut self, place: &Place<'ctxt>) -> ir::Place {
        match &place.kind {
            typed_ast::PlaceKind::Upvar(..) => todo!(),
            typed_ast::PlaceKind::Var(var) => {
                let local = *self
                    .vars
                    .get(&var.1)
                    .unwrap_or_else(|| panic!("should have a variable for {}", var.0));
                ir::Place::Local(local)
            }
            typed_ast::PlaceKind::Field(place, field_id) => {
                let place = self.lower_place(place);
                ir::Place::Field(Box::new(place), *field_id)
            }
            typed_ast::PlaceKind::Index(..) => todo!(),
            typed_ast::PlaceKind::Deref(..) => todo!(),
            typed_ast::PlaceKind::Invalid => todo!(),
        }
    }
    fn lower_expr_into(&mut self, dest: ir::Place, expr: &Expr<'ctxt>) {
        match &expr.kind {
            typed_ast::ExprKind::Unsafe(expr) => self.lower_expr_into(dest, expr),
            typed_ast::ExprKind::Return(expr) => {
                let Some(result) = self.lower_expr(expr) else {
                    return;
                };
                self.push_stmt(ir::Stmt::Return(result));
            }

            typed_ast::ExprKind::Panic => {
                self.push_stmt(ir::Stmt::Panic);
            }
            typed_ast::ExprKind::Block(block_body) => {
                for stmt in block_body.stmts.iter() {
                    self.lower_stmt(stmt);
                }
                self.lower_expr_into(dest, expr);
            }
            typed_ast::ExprKind::String(_)
            | typed_ast::ExprKind::Bool(_)
            | typed_ast::ExprKind::Int(_)
            | typed_ast::ExprKind::Unit
            | typed_ast::ExprKind::Err
            | typed_ast::ExprKind::Char(_)
            | typed_ast::ExprKind::Array(_)
            | typed_ast::ExprKind::BuiltinCall(..)
            | typed_ast::ExprKind::Lambda(_)
            | typed_ast::ExprKind::Tuple(_)
            | typed_ast::ExprKind::Function(..)
            | typed_ast::ExprKind::VariantInit(..)
            | typed_ast::ExprKind::NamedRecord(..)
            | typed_ast::ExprKind::Load(..)
            | typed_ast::ExprKind::Case(..) => {
                let result = self.lower_expr(expr);
                if let Some(result) = result {
                    self.push_stmt(ir::Stmt::Assign(dest, result));
                }
            }
            typed_ast::ExprKind::For { .. } | typed_ast::ExprKind::While(..) => todo!(),
            typed_ast::ExprKind::NeverToAny(..) => todo!("idk"),
            typed_ast::ExprKind::Assign(..) => todo!(),
            typed_ast::ExprKind::Call(callee, args) => self.lower_call(dest, callee, args),
            typed_ast::ExprKind::Logic(op, lhs, rhs) => {
                self.lower_logical(dest, *op, lhs, rhs);
            }
            typed_ast::ExprKind::Binary(..) => todo!("maybe"),
        }
    }
    fn lower_call(&mut self, dest: ir::Place, callee: &Expr<'ctxt>, args: &[Expr<'ctxt>]) {
        let Some(callee) = self.lower_expr(callee) else {
            return;
        };
        let Some(args) = args
            .iter()
            .map(|arg| self.lower_expr(arg))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let call = ir::Call {
            return_place: dest,
            callee: callee,
            args,
        };
        self.push_stmt(ir::Stmt::Call(call));
    }
    fn lower_logical(
        &mut self,
        dest: ir::Place,
        op: LogicalOp,
        lhs: &Expr<'ctxt>,
        rhs: &Expr<'ctxt>,
    ) {
        /*
           place = a or b;
           place = if a then true else b;
           if a then place = true else place = b;
        */
        let Some(lhs) = self.lower_expr(lhs) else {
            return;
        };
        let (rhs_stmts, ()) = self.stmts_for(|this| this.lower_expr_into(dest.clone(), rhs));
        let (true_branch, false_branch) = match op {
            LogicalOp::And => (
                rhs_stmts,
                vec![ir::Stmt::Assign(
                    dest,
                    ir::Expr::constant(ir::Constant::Bool(false)),
                )],
            ),
            LogicalOp::Or => (
                (vec![ir::Stmt::Assign(
                    dest,
                    ir::Expr::constant(ir::Constant::Bool(true)),
                )]),
                rhs_stmts,
            ),
        };
        self.push_stmt(ir::Stmt::If(lhs, true_branch, false_branch));
    }
    fn next_loop_label(&mut self) -> ir::LoopLabel {
        let label = ir::LoopLabel::new(self.loop_label as usize);
        self.loop_label += 1;
        label
    }
    fn in_loop<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (ir::LoopLabel, T) {
        let label = self.next_loop_label();
        let value = f(self);
        self.loop_label -= 1;
        (label, value)
    }
    fn lower_while_loop(&mut self, condition: &Expr<'ctxt>, body: &Expr<'ctxt>) {
        let Some(condition) = self.lower_expr(condition) else {
            return;
        };
        let (label, body) = self.in_loop(|this| {
            let (stmts, _) = this.stmts_for(|this| this.lower_expr(body));
            stmts
        });
        self.push_stmt(ir::Stmt::Loop(
            label,
            vec![ir::Stmt::If(condition, body, vec![ir::Stmt::Break(label)])],
        ));
    }
    fn lower_expr(&mut self, expr: &Expr<'ctxt>) -> Option<ir::Expr> {
        match &expr.kind {
            typed_ast::ExprKind::Unsafe(expr) => self.lower_expr(expr),
            typed_ast::ExprKind::Return(expr) => {
                let value = self.lower_expr(expr)?;
                self.push_stmt(ir::Stmt::Return(value));
                None
            }
            typed_ast::ExprKind::Block(block_body) => {
                for stmt in block_body.stmts.iter() {
                    self.lower_stmt(stmt);
                }
                self.lower_expr(&block_body.expr)
            }
            typed_ast::ExprKind::String(string) => Some(ir::Expr::constant(ir::Constant::String(
                Symbol::intern(string),
            ))),
            typed_ast::ExprKind::Bool(value) => {
                Some(ir::Expr::constant(ir::Constant::Bool(*value)))
            }
            typed_ast::ExprKind::Int(value) => Some(ir::Expr::constant(ir::Constant::Int(
                (*value).try_into().expect("too big"),
            ))),
            typed_ast::ExprKind::Char(_) => todo!("chars"),
            typed_ast::ExprKind::Unit => Some(ir::Expr::unit_value()),
            typed_ast::ExprKind::Err => unreachable!(),
            typed_ast::ExprKind::Panic => {
                self.push_stmt(ir::Stmt::Panic);
                None
            }
            typed_ast::ExprKind::NeverToAny(expr) => {
                let _ = self.lower_expr(expr);
                None
            }
            typed_ast::ExprKind::BuiltinCall(builtin, _, exprs) => match *builtin {
                Builtin::PrintString => {
                    let [value] = self.lower_exprs_const(exprs);
                    self.push_stmt(ir::Stmt::Print {
                        value,
                        is_err: false,
                    });
                    None
                }
                Builtin::EprintString => {
                    let [value] = self.lower_exprs_const(exprs);
                    self.push_stmt(ir::Stmt::Print {
                        value,
                        is_err: true,
                    });
                    None
                }
                Builtin::IntegerBuiltin(_) => todo!("integer builtins"),
                Builtin::Len => todo!("Array len"),
                Builtin::StringLen => todo!("String len"),
                Builtin::ReadLine => todo!("read len"),
            },
            typed_ast::ExprKind::VariantInit(..) => todo!(),
            typed_ast::ExprKind::Function(def_id, generic_args) => {
                assert!(generic_args.is_empty(), "Cant handle generics yet");
                Some(ir::Expr::constant(ir::Constant::Function(
                    self.ctxt.expect_body_id(*def_id),
                )))
            }
            typed_ast::ExprKind::Call(callee, args) => {
                let tmp = self.fresh_temp(lower_type(expr.ty));
                self.lower_call(ir::Place::Local(tmp), callee, args);
                Some(ir::Expr::load(ir::Place::Local(tmp)))
            }
            typed_ast::ExprKind::Load(place) => Some(ir::Expr::load(self.lower_place(place))),
            typed_ast::ExprKind::Binary(..) => todo!("Binary ops"),
            typed_ast::ExprKind::Logic(logical_op, lhs, rhs) => {
                let dest = self.fresh_temp(ir::Type::Bool);
                self.lower_logical(ir::Place::Local(dest), *logical_op, lhs, rhs);
                Some(ir::Expr::load(ir::Place::Local(dest)))
            }
            typed_ast::ExprKind::For { .. } => todo!(),
            typed_ast::ExprKind::Case(..) => todo!("case exprs"),
            typed_ast::ExprKind::Assign(place, rhs) => {
                let place = self.lower_place(place);
                self.lower_expr_into(place, rhs);
                Some(ir::Expr::unit_value())
            }
            typed_ast::ExprKind::Lambda(_) => todo!("lambdas"),
            typed_ast::ExprKind::Tuple(exprs) => Some(ir::Expr::tuple(
                exprs
                    .iter()
                    .map(|expr| self.lower_expr(expr))
                    .collect::<Option<Vec<_>>>()?,
            )),
            typed_ast::ExprKind::Array(_) => {
                todo!("arrays")
            }
            typed_ast::ExprKind::NamedRecord(..) => todo!("records"),
            typed_ast::ExprKind::While(condition, body) => {
                self.lower_while_loop(condition, body);
                Some(ir::Expr::unit_value())
            }
        }
    }
    pub fn lower(mut self) -> ir::Body {
        if let Some(ref expr) = self.function.body
            && let Some(result) = self.lower_expr(&expr)
        {
            self.push_stmt(ir::Stmt::Return(result));
        }
        let mut body = self.body;
        body.body.extend(self.stmts);
        body
    }
}
fn lower_type(ty: Type<'_>) -> ir::Type {
    match ty.kind() {
        TypeKind::Bool => ir::Type::Bool,
        TypeKind::Char => todo!("Chars"),
        TypeKind::Int => ir::Type::Int,
        TypeKind::Infer(_) => todo!(),
        TypeKind::Unknown => todo!(),
        TypeKind::IntVar(_) => todo!(),
        TypeKind::Never => todo!(),
        TypeKind::Param(_, index) => {
            ir::Type::Param((*index).try_into().expect("too many generic params"))
        }
        TypeKind::Function(function_sig) => ir::Type::Function(
            function_sig
                .params
                .iter()
                .copied()
                .map(lower_type)
                .collect(),
            Box::new(lower_type(function_sig.return_type)),
        ),
        TypeKind::Tuple(items) => ir::Type::Tuple(items.iter().copied().map(lower_type).collect()),
        TypeKind::Array(ty) => ir::Type::Array(Box::new(lower_type(*ty))),
        TypeKind::Named(..) => todo!(),
        TypeKind::String => ir::Type::String,
        TypeKind::Box(_) => todo!(),
    }
}
pub fn lower_program<'a, 'ctxt: 'a>(
    ctxt: CtxtRef<'ctxt>,
    functions: impl IntoIterator<Item = (DefId, &'a typed_ast::Function<'ctxt>)> + Clone,
) -> ir::Program {
    let id_map = functions
        .clone()
        .into_iter()
        .enumerate()
        .map(|(i, (id, _))| (id, BodyId::new(i)))
        .collect::<HashMap<_, _>>();
    let lowering_ctxt = LoweringCtxt { id_map };
    let program = ir::Program {
        bodies: functions
            .into_iter()
            .map(|(id, function)| LowerFunction::new(&lowering_ctxt, ctxt, id, function).lower())
            .collect(),
    };
    for body in &program.bodies {
        Print::new(&program, std::io::stdout()).print_body(body);
    }
    program
}
