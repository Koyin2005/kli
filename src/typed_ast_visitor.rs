use crate::typed_ast::{Expr, ExprKind, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind};

pub trait Visitor {
    fn visit_expr(&mut self, expr: &Expr) {
        walk_expr(self, expr);
    }
    fn visit_place(&mut self, place: &Place) {
        walk_place(self, place);
    }
    fn visit_pattern(&mut self, pattern: &Pattern) {
        walk_pattern(self, pattern);
    }
    fn visit_stmt(&mut self, stmt: &Stmt) {
        walk_stmt(self, stmt);
    }
}
pub fn walk_pattern<V>(v: &mut V, pattern: &Pattern)
where
    V: Visitor + ?Sized,
{
    match &pattern.kind {
        PatternKind::Binding(..)
        | PatternKind::Err
        | PatternKind::Bool(_)
        | PatternKind::Int(..)
        | PatternKind::Unit => (),
        PatternKind::Ref(pattern) => v.visit_pattern(pattern),
        PatternKind::Case(.., inner) => {
            if let Some(inner) = inner {
                v.visit_pattern(inner);
            }
        }
        PatternKind::Record(fields) => {
            for field in fields {
                v.visit_pattern(&field.pattern);
            }
        }
    }
}
pub fn walk_place<V>(v: &mut V, place: &Place)
where
    V: Visitor + ?Sized,
{
    match &place.kind {
        PlaceKind::Var(_) | PlaceKind::Upvar(..) | PlaceKind::Invalid => (),
        PlaceKind::Deref(value) => v.visit_expr(value),
        PlaceKind::Field(place, _) => v.visit_place(place),
    }
}
pub fn walk_stmt<V>(v: &mut V, stmt: &Stmt)
where
    V: Visitor + ?Sized,
{
    match &stmt.kind {
        StmtKind::Expr(expr) => {
            v.visit_expr(expr);
        }
        StmtKind::Let(let_binding) => {
            v.visit_pattern(&let_binding.pattern);
            v.visit_expr(&let_binding.value);
        }
    }
}
pub fn walk_expr<V>(v: &mut V, expr: &Expr)
where
    V: Visitor + ?Sized,
{
    match &expr.kind {
        ExprKind::Block(body, _) => {
            for stmt in &body.stmts {
                v.visit_stmt(stmt);
            }
            v.visit_expr(&body.expr);
        }
        ExprKind::Record(fields) | ExprKind::NamedRecord(.., fields) => {
            for field in fields {
                v.visit_expr(&field.value);
            }
        }
        ExprKind::AddressOf(place) => {
            v.visit_place(place);
        }
        ExprKind::Err
        | ExprKind::Int(_)
        | ExprKind::Const(..)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Function(..)
        | ExprKind::Unit
        | ExprKind::Panic => (),
        ExprKind::Print(value) => {
            if let Some(value) = value {
                v.visit_expr(value);
            }
        }
        ExprKind::BuiltinCall(_, _, exprs) => {
            for expr in exprs {
                v.visit_expr(expr);
            }
        }
        ExprKind::VariantInit(.., value) => v.visit_expr(value),
        ExprKind::Call(callee, args) => {
            v.visit_expr(callee);
            args.iter().for_each(|expr| v.visit_expr(expr));
        }
        ExprKind::List(values) => values.iter().for_each(|expr| v.visit_expr(expr)),
        ExprKind::Binary(_, first, second) | ExprKind::While(first, second) => {
            v.visit_expr(first);
            v.visit_expr(second)
        }
        ExprKind::Load(place) => v.visit_place(place),
        ExprKind::Assign(place, value) => {
            v.visit_place(place);
            v.visit_expr(value);
        }
        ExprKind::Borrow { place, .. } => v.visit_place(place),
        ExprKind::For {
            pattern,
            iterator,
            iterator_type: _,
            body,
        } => {
            v.visit_expr(iterator);
            v.visit_pattern(pattern);
            v.visit_expr(body);
        }
        ExprKind::Lambda(_) => {}
        ExprKind::Case(matched, arms) => {
            v.visit_expr(matched);
            for arm in arms {
                v.visit_pattern(&arm.pattern);
                v.visit_expr(&arm.body);
            }
        }
    }
}
