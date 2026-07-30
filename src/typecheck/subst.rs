use crate::{
    typecheck::infer::TypeInfer,
    typed_ast::{Expr, ExprKind, Function, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind},
    types::{GenericArgs, TypeKind, visit::VisitMut},
};

impl VisitMut for TypeSubst<'_> {
    fn visit_type(&mut self, ty: &mut TypeKind) {
        if let TypeKind::Infer(var) = ty {
            *ty = self.infer.simplify_type(TypeKind::Infer(*var));
            return;
        }
        self.super_visit_type(ty);
    }
}
pub struct TypeSubst<'a> {
    infer: &'a mut TypeInfer,
}
impl<'a> TypeSubst<'a> {
    pub fn new(infer: &'a mut TypeInfer) -> Self {
        Self { infer }
    }
    pub fn subst_type(&mut self, ty: &mut TypeKind) {
        self.visit_type(ty);
    }
    pub fn subst_generic_args(&mut self, args: &mut GenericArgs) {
        self.visit_generic_args(args);
    }
    pub fn subst_pattern(&mut self, pattern: &mut Pattern) {
        match &mut pattern.kind {
            PatternKind::Bool(_) | PatternKind::Int(_) | PatternKind::Err | PatternKind::Unit => (),
            PatternKind::Binding(.., ty) => self.subst_type(ty),
            PatternKind::Case(.., args, _, inner) => {
                self.subst_generic_args(args);
                if let Some(inner) = inner {
                    self.subst_pattern(inner);
                }
            }
            PatternKind::Record(fields) => {
                for field in fields {
                    self.subst_pattern(&mut field.pattern);
                }
            }
        }
        self.subst_type(&mut pattern.ty);
    }
    pub fn subst_place(&mut self, place: &mut Place) {
        match &mut place.kind {
            PlaceKind::Field(place, _) => self.subst_place(place),
            PlaceKind::Var(..) | PlaceKind::Upvar(..) | PlaceKind::Invalid => (),
            PlaceKind::Deref(expr) => self.subst_expr(expr),
            PlaceKind::Index(expr1, expr2) => {
                self.subst_expr(expr1);
                self.subst_expr(expr2);
            }
        }
        self.subst_type(&mut place.ty);
    }
    pub fn subst_stmt(&mut self, stmt: &mut Stmt) {
        match &mut stmt.kind {
            StmtKind::Expr(expr) => self.subst_expr(expr),
            StmtKind::Let(let_binding) => {
                self.subst_pattern(&mut let_binding.pattern);
                self.subst_expr(&mut let_binding.value);
            }
        }
    }
    pub fn subst_expr(&mut self, expr: &mut Expr) {
        match &mut expr.kind {
            ExprKind::Return(value) | ExprKind::Unsafe(value) => {
                self.subst_expr(value);
            }
            ExprKind::Block(block) => {
                for stmt in &mut block.stmts {
                    self.subst_stmt(stmt);
                }
                self.subst_expr(&mut block.expr);
            }
            ExprKind::Const(_, args) => {
                self.subst_generic_args(args);
            }
            ExprKind::NeverToAny(expr) => {
                self.subst_expr(expr);
            }
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::String(_)
            | ExprKind::Panic => (),
            ExprKind::Binary(_, first, second)
            | ExprKind::While(first, second)
            | ExprKind::Logic(_, first, second) => {
                self.subst_expr(first);
                self.subst_expr(second);
            }
            ExprKind::Print(expr) => {
                if let Some(expr) = expr {
                    self.subst_expr(expr);
                }
            }
            ExprKind::VariantInit(.., args, expr) => {
                self.subst_generic_args(args);
                if let Some(expr) = expr {
                    self.subst_expr(expr)
                }
            }
            ExprKind::Call(callee, args) => {
                self.subst_expr(callee);
                for arg in args {
                    self.subst_expr(arg);
                }
            }
            ExprKind::Load(place) => {
                self.subst_place(place);
            }
            ExprKind::For { iterator_type, .. } => match *iterator_type {},
            ExprKind::Assign(place, expr) => {
                self.subst_place(place);
                self.subst_expr(expr);
            }
            ExprKind::Case(matchee, arms) => {
                self.subst_expr(matchee);
                for arm in arms {
                    self.subst_pattern(&mut arm.pattern);
                    self.subst_expr(&mut arm.body);
                }
            }
            ExprKind::Function(.., args) => {
                self.subst_generic_args(args);
            }
            ExprKind::BuiltinCall(_, generic_args, args) => {
                self.subst_generic_args(generic_args);
                for expr in args {
                    self.subst_expr(expr);
                }
            }
            ExprKind::Lambda(lambda) => {
                for ty in lambda.param_tys.iter_mut() {
                    self.subst_type(ty);
                }
                self.subst_type(&mut lambda.return_type);
            }
            ExprKind::Record(fields) => {
                for field in fields {
                    self.subst_expr(&mut field.value);
                }
            }
            ExprKind::Tuple(fields) | ExprKind::Array(fields) => {
                for field in fields {
                    self.subst_expr(field);
                }
            }
            ExprKind::NamedRecord(_, args, fields) => {
                self.subst_generic_args(args);
                for field in fields {
                    self.subst_expr(&mut field.value);
                }
            }
        }
        self.subst_type(&mut expr.ty);
    }
    pub fn subst_function(&mut self, function: &mut Function) {
        for param in function.params.iter_mut() {
            self.subst_type(&mut param.ty);
        }
        self.subst_type(&mut function.return_type);
        if let Some(body) = function.body.as_mut() {
            self.subst_expr(body);
        }
    }
}
