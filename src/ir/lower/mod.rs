use std::collections::{BTreeMap, HashMap};

use crate::{
    CtxtRef, Symbol,
    ast::Mutable,
    builtins::{Builtin, IntegerBuiltin},
    collect::TypeDefKind,
    def_ids::DefId,
    ident::Ident,
    index_vec::IndexVec,
    ir::{self, BodyId, Local, TypeDef, TypeDefId, print::Print},
    resolved_ast::{Var, VarId},
    typed_ast::{
        self, BinaryOp, BlockBody, Expr, ExprKind, FieldId, LogicalOp, Pattern, PatternKind, Place,
        PlaceKind, Stmt, StmtKind,
    },
    types::{GenericArgs, Type, TypeKind},
};
enum BuiltinResult {
    Value(ir::Expr),
    Unit,
}
type Functions<'a, 'ctxt> = BTreeMap<DefId, &'a typed_ast::Function<'ctxt>>;
struct LoweringCtxt {
    id_map: HashMap<DefId, BodyId>,
    type_defs: HashMap<DefId, TypeDefId>,
    program: ir::Program,
}
impl LoweringCtxt {
    fn finish<'ctxt>(self, ctxt: CtxtRef<'ctxt>) -> ir::Program {
        let mut program = self.program;
        program.entrypoint = ctxt
            .main_function()
            .and_then(|(id, _)| self.id_map.get(&id).copied());
        program
    }
    fn type_def_id<'ctxt>(&mut self, id: DefId, ctxt: CtxtRef<'ctxt>) -> TypeDefId {
        if let Some(id) = self.type_defs.get(&id) {
            return *id;
        }
        let ty_id = {
            let type_def = match ctxt.type_def(id).kind {
                TypeDefKind::Record(_) => TypeDef::Struct,
                TypeDefKind::Variant(cases) => TypeDef::Variant(ir::VariantDef {
                    cases: cases
                        .into_iter()
                        .map(|case| ir::CaseDef {
                            name: case.name.to_string(),
                            field: case.field.map(|case| ir::CaseField {
                                ty: self.lower_type(ctxt.type_of(case.id).skip(), ctxt),
                            }),
                        })
                        .collect(),
                }),
            };
            self.program.type_defs.push(type_def)
        };
        self.type_defs.insert(id, ty_id);
        ty_id
    }
    fn lower_generic_args<'ctxt>(
        &mut self,
        args: &GenericArgs<'_>,
        ctxt: CtxtRef<'ctxt>,
    ) -> Vec<ir::Type> {
        args.iter()
            .map(|arg| self.lower_type(arg.expect_ty(), ctxt))
            .collect()
    }
    fn lower_type<'ctxt>(&mut self, ty: Type<'_>, ctxt: CtxtRef<'ctxt>) -> ir::Type {
        match ty.kind() {
            TypeKind::Bool => ir::Type::Bool,
            TypeKind::Char => ir::Type::Char,
            TypeKind::Int => ir::Type::Int,
            TypeKind::Infer(_) | TypeKind::Unknown | TypeKind::IntVar(_) => {
                unreachable!("types should be fully inferred, and have no errors")
            }
            TypeKind::Never => ir::Type::Never,
            TypeKind::Param(_, index) => {
                ir::Type::Param((*index).try_into().expect("too many generic params"))
            }
            TypeKind::Function(function_sig) => ir::Type::Function(
                function_sig
                    .params
                    .iter()
                    .copied()
                    .map(|ty| self.lower_type(ty, ctxt))
                    .collect(),
                Box::new(self.lower_type(function_sig.return_type, ctxt)),
            ),
            TypeKind::Tuple(items) => ir::Type::Tuple(
                items
                    .iter()
                    .copied()
                    .map(|ty| self.lower_type(ty, ctxt))
                    .collect(),
            ),
            TypeKind::Array(ty) => ir::Type::Array(Box::new(self.lower_type(*ty, ctxt))),
            TypeKind::Named(id, _, args) => {
                let id = self.type_def_id(*id, ctxt);
                ir::Type::Named(id, self.lower_generic_args(args, ctxt))
            }
            TypeKind::String => ir::Type::String,
            TypeKind::Box(ty) => ir::Type::Box(Box::new(self.lower_type(*ty, ctxt))),
        }
    }
}
pub(super) struct LowerFunction<'a, 'ctxt> {
    vars: HashMap<VarId, Local>,
    def_id: DefId,
    lower_ctxt: &'a mut LoweringCtxt,
    ctxt: CtxtRef<'ctxt>,
    body: ir::Body,
    stmts: Vec<ir::Stmt>,
    loop_label: u32,
    functions: &'a Functions<'a, 'ctxt>,
}

impl<'a, 'ctxt> LowerFunction<'a, 'ctxt> {
    fn new<'b>(
        lower_ctxt: &'a mut LoweringCtxt,
        ctxt: CtxtRef<'ctxt>,
        id: DefId,
        params: impl IntoIterator<Item = &'b typed_ast::Param<'ctxt>>,
        return_type: Type<'ctxt>,
        functions: &'a Functions<'a, 'ctxt>,
    ) -> Self
    where
        'ctxt: 'b,
    {
        let mut vars = HashMap::new();
        let mut locals = IndexVec::new();

        for (i, param) in params.into_iter().enumerate() {
            if let Some((var, local)) = param.var.map(|var| (var, Local::new(i))) {
                vars.insert(var, local);
            }
            locals.push(ir::LocalInfo {
                ty: lower_ctxt.lower_type(param.ty, ctxt),
                is_mutable: true,
                name: Some(param.name.symbol),
            });
        }

        Self {
            functions,
            def_id: id,
            loop_label: 0,
            vars,
            body: ir::Body {
                name: ctxt.expect_ident(id).symbol.to_string(),
                generic_params: ctxt.generics(id).names().collect(),
                param_count: locals.len() as u32,
                return_ty: lower_ctxt.lower_type(return_type, ctxt),
                locals,
                body: Vec::new(),
            },
            stmts: Vec::new(),
            ctxt,
            lower_ctxt,
        }
    }
    fn push_assign(&mut self, place: ir::Place, value: ir::Expr) {
        self.stmts.push(ir::Stmt::Assign(place, value));
    }
    fn push_tmp_assign(&mut self, ty: ir::Type, value: ir::Expr) {
        let local = self.fresh_temp(ty);
        self.push_assign(ir::Place::Local(local), value);
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
    fn lower_type(&mut self, ty: Type<'_>) -> ir::Type {
        self.lower_ctxt.lower_type(ty, self.ctxt)
    }
    fn lower_expr_to_place(&mut self, expr: &Expr<'ctxt>) -> ir::Place {
        if let Some(result) = self.lower_expr(expr) {
            if let ir::ExprKind::Load(place) = result.kind {
                place
            } else {
                let ty = self.lower_type(expr.ty);
                let tmp = self.fresh_temp(ty);
                self.push_stmt(ir::Stmt::Assign(ir::Place::Local(tmp), result));
                ir::Place::Local(tmp)
            }
        } else {
            let ty = self.lower_type(expr.ty);
            let tmp = self.fresh_temp(ty);
            ir::Place::Local(tmp)
        }
    }
    fn lower_builtin_call(&mut self, builtin: Builtin, args: &[Expr<'ctxt>]) -> BuiltinResult {
        match builtin {
            Builtin::PrintString => {
                let [value] = self.lower_exprs_const(args);
                self.push_stmt(ir::Stmt::Print {
                    value,
                    is_err: false,
                });
                BuiltinResult::Unit
            }
            Builtin::EprintString => {
                let [value] = self.lower_exprs_const(args);
                self.push_stmt(ir::Stmt::Print {
                    value,
                    is_err: true,
                });
                BuiltinResult::Unit
            }
            Builtin::IntegerBuiltin(builtin) => match builtin {
                IntegerBuiltin::IntMaxValue => {
                    BuiltinResult::Value(ir::Expr::constant(ir::Constant::Int(i64::MAX)))
                }
                IntegerBuiltin::WrappingAdd => {
                    let [left, right] = self.lower_exprs_const(args);
                    BuiltinResult::Value(ir::Expr::binary(ir::BinaryOp::Add, left, right))
                }
                IntegerBuiltin::OverflowingAdd => {
                    let [left, right] = self.lower_exprs_const(args);
                    BuiltinResult::Value(ir::Expr::binary(
                        ir::BinaryOp::AddWithOverflow,
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::ShiftLeft => todo!("shift left"),
                IntegerBuiltin::ShiftRight => todo!("shift right"),
                IntegerBuiltin::WrappingSub => {
                    let [left, right] = self.lower_exprs_const(args);
                    BuiltinResult::Value(ir::Expr::binary(ir::BinaryOp::Subtract, left, right))
                }
                IntegerBuiltin::OverflowingSub => {
                    let [left, right] = self.lower_exprs_const(args);
                    BuiltinResult::Value(ir::Expr::binary(
                        ir::BinaryOp::SubtractWithOverflow,
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::WrappingMul => {
                    let [left, right] = self.lower_exprs_const(args);
                    BuiltinResult::Value(ir::Expr::binary(ir::BinaryOp::Multiply, left, right))
                }
                IntegerBuiltin::OverflowingMul => {
                    let [left, right] = self.lower_exprs_const(args);
                    BuiltinResult::Value(ir::Expr::binary(
                        ir::BinaryOp::MultiplyWithOverflow,
                        left,
                        right,
                    ))
                }
            },
            Builtin::Len => BuiltinResult::Value(ir::Expr::len({
                let [expr] = self.lower_exprs_const(args);
                expr
            })),
            Builtin::StringLen => todo!("String len"),
            Builtin::ReadLine => {
                let (tmp, ()) = self.lower_into_temp(ir::Type::String, |tmp, this| {
                    this.push_stmt(ir::Stmt::ReadLine(ir::Place::Local(tmp)));
                });
                BuiltinResult::Value(ir::Expr::load_local(tmp))
            }
        }
    }
    fn lower_expr_stmt(&mut self, expr: &Expr<'ctxt>) {
        match &expr.kind {
            ExprKind::Unit => (),
            ExprKind::Err => unreachable!(),
            ExprKind::Unsafe(expr) => self.lower_expr_stmt(expr),
            ExprKind::Return(expr) => {
                let Some(result) = self.lower_expr(expr) else {
                    return;
                };
                self.push_stmt(ir::Stmt::Return(result));
            }
            ExprKind::If(condition, then_branch, else_branch) => {
                let Some(condition) = self.lower_expr(condition) else {
                    return;
                };
                let (then_stmts, ()) = self.stmts_for(|this| {
                    this.lower_expr_stmt(then_branch);
                });
                let (else_stmts, ()) = self.stmts_for(|this| {
                    this.lower_expr_stmt(else_branch);
                });
                self.push_stmt(ir::Stmt::If(condition, then_stmts, else_stmts));
            }
            ExprKind::Block(block_body) => {
                self.lower_block_stmts(block_body);
                self.lower_expr_stmt(&block_body.expr);
            }
            ExprKind::Assign(place, rhs) => {
                self.lower_assign(place, rhs);
            }
            ExprKind::Panic => {
                self.push_stmt(ir::Stmt::Panic);
            }
            ExprKind::NeverToAny(expr) => {
                self.lower_expr_stmt(expr);
            }
            ExprKind::BuiltinCall(builtin, _, args) => {
                match self.lower_builtin_call(*builtin, args) {
                    BuiltinResult::Unit => (),
                    BuiltinResult::Value(value) => {
                        let ty = self.lower_type(expr.ty);
                        self.push_tmp_assign(ty, value);
                    }
                }
            }
            ExprKind::While(condition, body) => {
                self.lower_while_loop(condition, body);
            }
            ExprKind::String(_)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Char(_)
            | ExprKind::Function(..)
            | ExprKind::VariantConstructor { .. }
            | ExprKind::Case(..)
            | ExprKind::Call(..)
            | ExprKind::Load(_)
            | ExprKind::Lambda(_)
            | ExprKind::Binary(..)
            | ExprKind::Logic(..)
            | ExprKind::Tuple(..)
            | ExprKind::Array(..)
            | ExprKind::NamedRecord(..)
            | ExprKind::VariantInit(..) => {
                self.lower_expr_to_temp(expr);
            }
        }
    }
    fn lower_stmt(&mut self, stmt: &Stmt<'ctxt>) {
        match &stmt.kind {
            StmtKind::Expr(expr) => {
                self.lower_expr_stmt(expr);
            }
            StmtKind::Let(binding) => {
                self.assign_to_pattern(&binding.pattern, &binding.value);
            }
        }
    }
    fn assign_to_pattern(&mut self, pattern: &Pattern<'ctxt>, expr: &Expr<'ctxt>) {
        match pattern.kind {
            PatternKind::Binding(mutable, var, ty) => {
                let ty = self.lower_type(ty);
                let local = self.fresh_local_for_var(var, matches!(mutable, Mutable::Mutable), ty);
                self.lower_expr_into(ir::Place::Local(local), expr);
            }
            PatternKind::Unit
            | PatternKind::Bool(_)
            | PatternKind::Int(_)
            | PatternKind::Char(_) => (),
            PatternKind::Err => unreachable!("cannot assign to err patterns"),
            _ => {
                let result = self.lower_expr_to_place(&expr);
                self.lower_place_to_pattern(result, pattern);
            }
        }
    }
    fn lower_place_to_pattern(&mut self, place: ir::Place, pattern: &Pattern<'ctxt>) {
        match &pattern.kind {
            PatternKind::Binding(mutable, var, ty) => {
                let ty = self.lower_type(*ty);
                let local = self.fresh_local_for_var(*var, matches!(mutable, Mutable::Mutable), ty);
                self.push_stmt(ir::Stmt::Assign(
                    ir::Place::Local(local),
                    ir::Expr::load(place),
                ));
            }
            PatternKind::Err => unreachable!(),
            PatternKind::Unit
            | PatternKind::Int(_)
            | PatternKind::Bool(_)
            | PatternKind::Char(_) => (),
            PatternKind::Case(_, _, case_id, field) => {
                if let Some(field) = field {
                    self.lower_place_to_pattern(
                        ir::Place::Downcast(Box::new(place.clone()), *case_id)
                            .with_field(FieldId::new(0)),
                        field,
                    );
                }
            }
            PatternKind::Record(fields) => {
                for field in fields {
                    self.lower_place_to_pattern(
                        ir::Place::Field(Box::new(place.clone()), field.index),
                        &field.pattern,
                    );
                }
            }
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
    fn local_for_var(&self, var: Var) -> Local {
        *self
            .vars
            .get(&var.1)
            .unwrap_or_else(|| panic!("should have a variable for {}", var.0))
    }
    fn lower_index(&mut self, index: &Expr<'ctxt>) -> Option<ir::Expr> {
        match &index.kind {
            ExprKind::Load(place) => {
                if let PlaceKind::Index(..) = place.kind {
                    Some(ir::Expr::load_local(self.lower_expr_to_temp(index)))
                } else {
                    Some(ir::Expr::load(self.lower_place(place)?))
                }
            }
            &ExprKind::Int(value) => Some(ir::Expr::constant(ir::Constant::Int(
                value.try_into().expect("too big"),
            ))),
            _ => {
                let index = self.lower_expr_to_temp(index);
                Some(ir::Expr::load_local(index))
            }
        }
    }
    fn lower_place(&mut self, place: &Place<'ctxt>) -> Option<ir::Place> {
        match &place.kind {
            typed_ast::PlaceKind::Upvar(..) => todo!(),
            typed_ast::PlaceKind::Var(var) => Some(ir::Place::Local(self.local_for_var(*var))),
            typed_ast::PlaceKind::Field(place, field_id) => {
                let place = self.lower_place(place)?;
                Some(ir::Place::Field(Box::new(place), *field_id))
            }
            typed_ast::PlaceKind::Index(base, index) => {
                let base = self.lower_expr_to_place(base);
                let index = self.lower_index(index)?;
                self.bounds_check(ir::Expr::load(base.clone()), index.clone());
                Some(ir::Place::Index(Box::new(base), Box::new(index)))
            }
            typed_ast::PlaceKind::Deref(expr) => {
                let place = self.lower_expr_to_place(expr);
                Some(ir::Place::Deref(Box::new(place)))
            }
            typed_ast::PlaceKind::Invalid => unreachable!(),
        }
    }
    fn lower_array(&mut self, dest: ir::Place, ty: Type<'_>, elements: &[Expr<'ctxt>]) {
        let Some(elements) = elements
            .iter()
            .map(|element| self.lower_expr(element))
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let ty = self.lower_type(ty);
        self.push_stmt(ir::Stmt::Alloc(dest, ir::Allocate::Array(ty, elements)));
    }
    fn lower_if_expr(
        &mut self,
        dest: ir::Place,
        condition: &Expr<'ctxt>,
        then_branch: &Expr<'ctxt>,
        else_branch: &Expr<'ctxt>,
    ) {
        let Some(condition) = self.lower_expr(condition) else {
            return;
        };
        let (then_stmts, ()) = self.stmts_for(|this| {
            this.lower_expr_into(dest.clone(), then_branch);
        });
        let (else_stmts, ()) = self.stmts_for(|this| {
            this.lower_expr_into(dest.clone(), else_branch);
        });
        self.push_stmt(ir::Stmt::If(condition, then_stmts, else_stmts));
    }
    fn lower_into_temp<T>(
        &mut self,
        ty: ir::Type,
        f: impl FnOnce(Local, &mut Self) -> T,
    ) -> (Local, T) {
        let local = self.fresh_temp(ty);
        let value = f(local, self);
        (local, value)
    }
    fn lower_expr_to_temp(&mut self, expr: &Expr<'ctxt>) -> ir::Local {
        let ty = self.lower_type(expr.ty);
        let (index, ()) = self.lower_into_temp(ty, |local, this| {
            this.lower_expr_into(ir::Place::Local(local), expr);
        });
        index
    }
    fn bounds_check(&mut self, base: ir::Expr, index: ir::Expr) {
        let in_bounds = ir::Expr::binary(ir::BinaryOp::InBounds, index, ir::Expr::len(base));
        self.push_stmt(ir::Stmt::PanicIf(ir::Expr::not(in_bounds)));
    }
    fn lower_assign(&mut self, place: &Place<'ctxt>, rhs: &Expr<'ctxt>) {
        match &place.kind {
            PlaceKind::Var(_)
            | PlaceKind::Field(..)
            | PlaceKind::Deref(_)
            | PlaceKind::Upvar(..)
            | PlaceKind::Invalid => {
                let Some(place) = self.lower_place(place) else {
                    return;
                };
                self.lower_expr_into(place, rhs);
            }
            PlaceKind::Index(base, index) => {
                let base = self.lower_expr_to_place(base);
                let Some(index) = self.lower_index(index) else {
                    return;
                };
                let Some(rhs) = self.lower_expr(rhs) else {
                    return;
                };
                let place = ir::Place::Index(Box::new(base.clone()), Box::new(index.clone()));
                self.bounds_check(ir::Expr::load(base), index);
                self.push_stmt(ir::Stmt::Assign(place, rhs));
            }
        }
    }
    fn lower_block_stmts(&mut self, body: &BlockBody<'ctxt>) {
        for stmt in body.stmts.iter() {
            self.lower_stmt(stmt);
        }
    }
    fn lower_expr_into(&mut self, dest: ir::Place, expr: &Expr<'ctxt>) {
        match &expr.kind {
            typed_ast::ExprKind::If(condition, then_branch, else_branch) => {
                self.lower_if_expr(dest, condition, then_branch, else_branch);
            }
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
                self.lower_block_stmts(block_body);
                self.lower_expr_into(dest, &block_body.expr);
            }
            typed_ast::ExprKind::Array(elements) => {
                let ty = expr.ty.as_array().expect("should be an array");
                self.lower_array(dest, ty, elements);
            }
            typed_ast::ExprKind::String(_)
            | typed_ast::ExprKind::Bool(_)
            | typed_ast::ExprKind::Int(_)
            | typed_ast::ExprKind::Unit
            | typed_ast::ExprKind::Err
            | typed_ast::ExprKind::Char(_)
            | typed_ast::ExprKind::BuiltinCall(..)
            | typed_ast::ExprKind::Lambda(_)
            | typed_ast::ExprKind::Tuple(_)
            | typed_ast::ExprKind::Function(..)
            | typed_ast::ExprKind::VariantInit(..)
            | typed_ast::ExprKind::NamedRecord(..)
            | typed_ast::ExprKind::Load(..)
            | typed_ast::ExprKind::Case(..)
            | typed_ast::ExprKind::Binary(..)
            | typed_ast::ExprKind::Assign(..)
            | typed_ast::ExprKind::VariantConstructor { .. }
            | typed_ast::ExprKind::While(..)
            | typed_ast::ExprKind::NeverToAny(_) => {
                let result = self.lower_expr(expr);
                if let Some(result) = result {
                    self.push_stmt(ir::Stmt::Assign(dest, result));
                }
            }
            typed_ast::ExprKind::Call(callee, args) => self.lower_call(dest, callee, args),
            typed_ast::ExprKind::Logic(op, lhs, rhs) => {
                self.lower_logical(dest, *op, lhs, rhs);
            }
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
        let (label, (body, condition)) = self.in_loop(|this| {
            let (stmts, condition) = this.stmts_for(|this| {
                let condition = this.lower_expr(condition)?;
                this.lower_expr_stmt(body);
                Some(condition)
            });
            (stmts, condition)
        });
        if let Some(condition) = condition {
            self.push_stmt(ir::Stmt::Loop(
                label,
                vec![ir::Stmt::If(condition, body, vec![ir::Stmt::Break(label)])],
            ));
        }
    }
    fn checked_overflow_op_expr(
        &mut self,
        op: ir::OverflowOp,
        left: ir::Expr,
        right: ir::Expr,
    ) -> ir::Expr {
        let op = match op {
            ir::OverflowOp::Add => ir::BinaryOp::AddWithOverflow,
            ir::OverflowOp::Sub => ir::BinaryOp::SubtractWithOverflow,
            ir::OverflowOp::Mul => ir::BinaryOp::MultiplyWithOverflow,
        };
        let tmp = self.fresh_temp(ir::Type::Tuple(vec![ir::Type::Int, ir::Type::Bool]));
        let result = ir::Expr::binary(op, left, right);
        self.push_stmt(ir::Stmt::Assign(ir::Place::Local(tmp), result));
        self.push_stmt(ir::Stmt::PanicIf(ir::Expr::load(
            ir::Place::Local(tmp).with_field(ir::FieldId::new(1)),
        )));
        ir::Expr::load(ir::Place::Local(tmp).with_field(ir::FieldId::new(0)))
    }
    fn lower_binary_op_expr(
        &mut self,
        op: BinaryOp,
        left: &Expr<'ctxt>,
        right: &Expr<'ctxt>,
    ) -> Option<ir::Expr> {
        let left = self.lower_expr(left)?;
        let right = self.lower_expr(right)?;
        let op = match op {
            BinaryOp::Add => {
                return Some(self.checked_overflow_op_expr(ir::OverflowOp::Add, left, right));
            }
            BinaryOp::Subtract => {
                return Some(self.checked_overflow_op_expr(ir::OverflowOp::Sub, left, right));
            }
            BinaryOp::Multiply => {
                return Some(self.checked_overflow_op_expr(ir::OverflowOp::Mul, left, right));
            }
            BinaryOp::Lesser => ir::BinaryOp::Lesser,
            BinaryOp::Equals => ir::BinaryOp::Equals,
            BinaryOp::Greater => ir::BinaryOp::Greater,
            op => todo!("binary op {:?}", op),
        };
        Some(ir::Expr::binary(op, left, right))
    }
    fn lower_expr(&mut self, expr: &Expr<'ctxt>) -> Option<ir::Expr> {
        match &expr.kind {
            typed_ast::ExprKind::Unsafe(inner) => self.lower_expr(inner),
            typed_ast::ExprKind::Return(inner) => {
                let value = self.lower_expr(inner)?;
                self.push_stmt(ir::Stmt::Return(value));
                None
            }
            typed_ast::ExprKind::Block(block_body) => {
                self.lower_block_stmts(block_body);
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
            typed_ast::ExprKind::Char(value) => {
                Some(ir::Expr::constant(ir::Constant::Char(*value)))
            }
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
            typed_ast::ExprKind::If(condition, then_branch, else_branch) => {
                let ty = self.lower_type(expr.ty);
                let (dest, ()) = self.lower_into_temp(ty, |dest, this| {
                    this.lower_if_expr(ir::Place::Local(dest), condition, then_branch, else_branch)
                });
                Some(ir::Expr::load_local(dest))
            }
            typed_ast::ExprKind::BuiltinCall(builtin, _, exprs) => {
                match self.lower_builtin_call(*builtin, exprs) {
                    BuiltinResult::Unit => Some(ir::Expr::unit_value()),
                    BuiltinResult::Value(value) => Some(value),
                }
            }
            typed_ast::ExprKind::VariantInit(def_id, case_id, args, field) => Some({
                let ty_id = self.lower_ctxt.type_def_id(*def_id, self.ctxt);
                let args = self.lower_ctxt.lower_generic_args(args, self.ctxt);
                ir::Expr::aggregate(
                    ir::AggregateKind::Variant(ty_id, *case_id, args),
                    if let Some(field) = field {
                        Some(self.lower_expr(field)?)
                    } else {
                        None
                    },
                )
            }),
            typed_ast::ExprKind::Function(def_id, generic_args) => {
                let body_id = if let Some(&id) = self.lower_ctxt.id_map.get(def_id) {
                    id
                } else {
                    let function = self.functions[def_id];
                    LowerFunction::new(
                        self.lower_ctxt,
                        self.ctxt,
                        *def_id,
                        function.params.iter(),
                        function.return_type,
                        self.functions,
                    )
                    .lower(function.body.as_ref())
                };
                let args = self.lower_ctxt.lower_generic_args(generic_args, self.ctxt);
                Some(ir::Expr::constant(ir::Constant::Function(body_id, args)))
            }
            typed_ast::ExprKind::VariantConstructor { ty, args, case } => {
                let type_def = self.ctxt.type_def(*ty);
                let name = type_def.name;
                let case_info = type_def.case(*case);
                let id = case_info.id;
                let body_id = if let Some(&id) = self.lower_ctxt.id_map.get(&id) {
                    id
                } else {
                    let args = self.ctxt.generics(id).instantiate_identity(self.ctxt);
                    let params = case_info
                        .field
                        .iter()
                        .map(|field| {
                            let ty = field.type_of(&args, self.ctxt);
                            typed_ast::Param {
                                name: Ident::new(field.name, self.ctxt.span(field.id)),
                                ty,
                                var: None,
                            }
                        })
                        .collect::<Vec<_>>();
                    let param_count = params.len();
                    let mut lower_body = LowerFunction::new(
                        self.lower_ctxt,
                        self.ctxt,
                        id,
                        params.iter(),
                        Type::named(self.ctxt, *ty, name, args.clone()),
                        self.functions,
                    );
                    let return_value = ir::Expr::aggregate(
                        ir::AggregateKind::Variant(
                            lower_body.lower_ctxt.type_def_id(*ty, self.ctxt),
                            *case,
                            lower_body.lower_ctxt.lower_generic_args(&args, self.ctxt),
                        ),
                        (0..param_count).map(|i| ir::Expr::load_local(ir::Local::new(i))),
                    );
                    lower_body.push_stmt(ir::Stmt::Return(return_value));
                    lower_body.finish()
                };
                Some(ir::Expr::constant(ir::Constant::Function(
                    body_id,
                    self.lower_ctxt.lower_generic_args(args, self.ctxt),
                )))
            }
            typed_ast::ExprKind::Call(callee, args) => {
                let ty = self.lower_type(expr.ty);
                let (dest, ()) = self.lower_into_temp(ty, |dest, this| {
                    this.lower_call(ir::Place::Local(dest), callee, args)
                });
                Some(ir::Expr::load_local(dest))
            }
            typed_ast::ExprKind::Load(place) => Some(ir::Expr::load(self.lower_place(place)?)),
            typed_ast::ExprKind::Binary(op, left, right) => {
                self.lower_binary_op_expr(*op, left, right)
            }
            typed_ast::ExprKind::Logic(logical_op, lhs, rhs) => {
                let (dest, ()) = self.lower_into_temp(ir::Type::Bool, |dest, this| {
                    this.lower_logical(ir::Place::Local(dest), *logical_op, lhs, rhs)
                });
                Some(ir::Expr::load_local(dest))
            }
            typed_ast::ExprKind::Case(..) => todo!("case exprs"),
            typed_ast::ExprKind::Assign(place, rhs) => {
                self.lower_assign(place, rhs);
                Some(ir::Expr::unit_value())
            }
            typed_ast::ExprKind::Lambda(_) => todo!("lambdas"),
            typed_ast::ExprKind::Tuple(exprs) => Some(ir::Expr::tuple(
                exprs
                    .iter()
                    .map(|expr| self.lower_expr(expr))
                    .collect::<Option<Vec<_>>>()?,
            )),
            typed_ast::ExprKind::Array(elements) => {
                let element_type = expr.ty.as_array().expect("should be an array");
                let ty = self.lower_type(expr.ty);
                let (dest, ()) = self.lower_into_temp(ty, |dest, this| {
                    this.lower_array(ir::Place::Local(dest), element_type, elements);
                });
                Some(ir::Expr::load_local(dest))
            }
            typed_ast::ExprKind::NamedRecord(_, generic_args, fields) => {
                if !generic_args.is_empty() {
                    todo!("handle generic args")
                }
                let mut field_map = fields
                    .iter()
                    .map(|field_init| Some((field_init.index, self.lower_expr(&field_init.value)?)))
                    .collect::<Option<HashMap<_, _>>>()?;
                Some(ir::Expr::aggregate(
                    ir::AggregateKind::Named,
                    (0..fields.len()).map(|field| {
                        field_map
                            .remove(&FieldId::new(field))
                            .expect("should have a value for this field")
                    }),
                ))
            }
            typed_ast::ExprKind::While(condition, body) => {
                self.lower_while_loop(condition, body);
                Some(ir::Expr::unit_value())
            }
        }
    }
    pub fn finish(self) -> BodyId {
        let mut body = self.body;
        body.body.extend(self.stmts);
        let body_id = self.lower_ctxt.program.bodies.push(body);
        self.lower_ctxt.id_map.insert(self.def_id, body_id);
        body_id
    }
    pub fn lower(mut self, body: Option<&'_ typed_ast::Expr<'ctxt>>) -> BodyId {
        if let Some(expr) = body
            && let Some(result) = self.lower_expr(&expr)
        {
            self.push_stmt(ir::Stmt::Return(result));
        }
        self.finish()
    }
}
pub fn lower_program<'a, 'ctxt: 'a>(
    ctxt: CtxtRef<'ctxt>,
    functions: Functions<'a, 'ctxt>,
) -> ir::Program {
    let mut lowering_ctxt = LoweringCtxt {
        id_map: HashMap::new(),
        type_defs: HashMap::new(),
        program: ir::Program::default(),
    };

    for (&id, &function) in &functions {
        if lowering_ctxt.id_map.contains_key(&id) {
            continue;
        }
        LowerFunction::new(
            &mut lowering_ctxt,
            ctxt,
            id,
            function.params.iter(),
            function.return_type,
            &functions,
        )
        .lower(function.body.as_ref());
    }
    let program = lowering_ctxt.finish(ctxt);
    for body in &program.bodies {
        Print::new(&program, std::io::stdout()).print_body(body);
    }
    program
}
