use crate::resolved_ast::{
    BlockBody, Expr, ExprKind, GenericArg, GenericArgs, LocalRegionId, Param, Pattern, Region,
    Stmt, StmtKind, Type, TypeKind, Var,
};

pub trait Visitor {
    fn super_visit_block(&mut self, block_body: &BlockBody, _: Option<LocalRegionId>) {
        for stmt in &block_body.stmts {
            self.visit_stmt(stmt);
        }
        self.visit_expr(&block_body.expr);
    }
    fn super_visit_pattern(&mut self, pattern: &Pattern) {
        match &pattern.kind {
            crate::resolved_ast::PatternKind::Unit
            | crate::resolved_ast::PatternKind::Int(_)
            | crate::resolved_ast::PatternKind::Bool(_) => {}
            &crate::resolved_ast::PatternKind::Binding(.., name, var) => {
                self.visit_var_def(Var(name.symbol, var))
            }
            crate::resolved_ast::PatternKind::Ref(pattern) => self.visit_pattern(pattern),
            crate::resolved_ast::PatternKind::Case(_, pattern) => {
                if let Some(pattern) = pattern {
                    self.visit_pattern(pattern)
                }
            }
            crate::resolved_ast::PatternKind::Record(pattern_fields) => {
                for field in pattern_fields {
                    self.visit_pattern(&field.pattern);
                }
            }
            crate::resolved_ast::PatternKind::Tuple(pattern_fields) => {
                for field in pattern_fields {
                    self.visit_pattern(field);
                }
            }
        }
    }
    fn super_visit_type(&mut self, ty: &Type) {
        match &ty.kind {
            TypeKind::Unknown => (),
            TypeKind::Function(function_type) => {
                for param in function_type.params.iter() {
                    self.visit_type(param);
                }
                self.visit_type(&function_type.return_type);
            }
            TypeKind::Named(_, generic_args) => self.visit_generic_args(generic_args),
            TypeKind::Record(record_field_types) => {
                for field in record_field_types {
                    self.visit_type(&field.ty);
                }
            }
            TypeKind::Tuple(fields) => {
                for field in fields {
                    self.visit_type(field);
                }
            }
        }
    }
    fn super_visit_stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Let(let_binding) => {
                if let Some(ref ty) = let_binding.ty {
                    self.visit_type(ty);
                }
                self.visit_expr(&let_binding.value);
                self.visit_pattern(&let_binding.pattern);
            }
            StmtKind::Expr(expr) => self.visit_expr(expr),
        }
    }
    fn super_visit_expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Block(block_body, id) => self.visit_block(block_body, *id),
            ExprKind::Unit
            | ExprKind::Err
            | ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::String(_)
            | ExprKind::Var(..)
            | ExprKind::Panic => (),
            ExprKind::Lambda(lambda) => {
                self.visit_body(
                    lambda.param_tys.iter().flatten(),
                    lambda.params.iter(),
                    &lambda.body,
                );
            }
            ExprKind::Annotate(expr, ty) => {
                self.visit_expr(expr);
                self.visit_type(ty);
            }
            ExprKind::Function(_, args) | ExprKind::TypeRelativePath(_, _, args) => {
                self.visit_generic_args(args);
            }
            ExprKind::Binary(_, expr1, expr2) | ExprKind::While(expr1, expr2) => {
                self.visit_expr(expr1);
                self.visit_expr(expr2);
            }
            ExprKind::Deref(expr)
            | ExprKind::Unsafe(expr)
            | ExprKind::Field(expr, _)
            | ExprKind::Return(expr) => self.visit_expr(expr),
            ExprKind::Assign(place, expr) => {
                self.visit_expr(place);
                self.visit_expr(expr);
            }
            ExprKind::For(for_expr) => {
                self.visit_expr(&for_expr.iterator);
                self.visit_pattern(&for_expr.pattern);
                self.visit_expr(&for_expr.body);
            }
            ExprKind::Case(expr, case_arms) => {
                self.visit_expr(expr);
                for arm in case_arms {
                    self.visit_pattern(&arm.pattern);
                    self.visit_expr(&arm.body);
                }
            }
            ExprKind::Print(expr) => {
                if let Some(expr) = expr {
                    self.visit_expr(expr);
                }
            }
            ExprKind::Call(callee, args) | ExprKind::MethodCall(callee, _, args) => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            ExprKind::Record(field_inits) => {
                for field in field_inits {
                    self.visit_expr(&field.value);
                }
            }
            ExprKind::NamedRecord(_, generic_args, field_inits) => {
                self.visit_generic_args(generic_args);
                for field in field_inits {
                    self.visit_expr(&field.value);
                }
            }
            ExprKind::Tuple(fields) => {
                for field in fields {
                    self.visit_expr(field);
                }
            }
            ExprKind::VariantCase(_, generic_args) => self.visit_generic_args(generic_args),
            ExprKind::AddressOf(place) => self.visit_expr(place),
        }
    }
    fn visit_pattern(&mut self, pattern: &Pattern) {
        self.super_visit_pattern(pattern);
    }
    fn visit_var_def(&mut self, _: Var) {}
    fn visit_generic_args(&mut self, args: &GenericArgs) {
        for arg in args.args.iter() {
            match arg {
                GenericArg::Region(region) => self.visit_region(*region),
                GenericArg::Type(ty) => self.visit_type(ty),
            }
        }
    }
    fn visit_region(&mut self, _: Region) {}
    fn visit_block(&mut self, block_body: &BlockBody, region: Option<LocalRegionId>) {
        self.super_visit_block(block_body, region);
    }
    fn visit_type(&mut self, ty: &Type) {
        self.super_visit_type(ty);
    }
    fn visit_stmt(&mut self, stmt: &Stmt) {
        self.super_visit_stmt(stmt);
    }
    fn visit_expr(&mut self, expr: &Expr) {
        self.super_visit_expr(expr);
    }
    fn visit_body<'a>(
        &mut self,
        tys: impl Iterator<Item = &'a Type>,
        params: impl Iterator<Item = &'a Param>,
        body: &Expr,
    ) {
        for param in params {
            self.visit_var_def(param.var);
        }
        for ty in tys {
            self.visit_type(ty);
        }
        self.visit_expr(body);
    }
}
