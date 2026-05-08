use crate::typed_ast::{Expr, ExprKind, Pattern, PatternKind, Place, PlaceKind};

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
}
pub fn walk_pattern<V>(v: &mut V, pattern: &Pattern)
where
    V: Visitor + ?Sized,
{
    match &pattern.kind {
        PatternKind::Binding(..) | PatternKind::Bool(_) | PatternKind::None => (),
        PatternKind::Deref(pattern) | PatternKind::Some(pattern) => v.visit_pattern(pattern),
    }
}
pub fn walk_place<V>(v: &mut V, place: &Place)
where
    V: Visitor + ?Sized,
{
    match &place.kind {
        PlaceKind::Var(_) => (),
        PlaceKind::Deref(value) => v.visit_expr(value),
    }
}
pub fn walk_expr<V>(v: &mut V, expr: &Expr)
where
    V: Visitor + ?Sized,
{
    match &expr.kind {
        ExprKind::Err
        | ExprKind::Int(_)
        | ExprKind::Bool(_)
        | ExprKind::Builtin(..)
        | ExprKind::String(_)
        | ExprKind::Function(..)
        | ExprKind::Unit
        | ExprKind::Panic
        | ExprKind::None => (),
        ExprKind::Print(value) => {
            if let Some(value) = value {
                v.visit_expr(value);
            }
        }
        ExprKind::Some(value) => v.visit_expr(value),
        ExprKind::Call(callee, args) => {
            v.visit_expr(callee);
            args.iter().for_each(|expr| v.visit_expr(expr));
        }
        ExprKind::List(values) => values.iter().for_each(|expr| v.visit_expr(expr)),
        ExprKind::Binary(_, first, second) | ExprKind::Sequence(first, second) => {
            v.visit_expr(first);
            v.visit_expr(second)
        }
        ExprKind::Load(place) => v.visit_place(place),
        ExprKind::Assign(place, value) => {
            v.visit_place(place);
            v.visit_expr(value);
        }
        ExprKind::Borrow { body, .. } => v.visit_expr(body),
        ExprKind::For {
            pattern,
            iterator,
            body,
        } => {
            v.visit_expr(iterator);
            v.visit_pattern(pattern);
            v.visit_expr(body);
        }
        ExprKind::Lambda(lambda) => {
            v.visit_expr(&lambda.body);
        }
        ExprKind::Case(matched, arms) => {
            v.visit_expr(matched);
            for arm in arms {
                v.visit_pattern(&arm.pattern);
                v.visit_expr(&arm.body);
            }
        }
        ExprKind::Let {
            pattern,
            binder,
            body,
        } => {
            v.visit_expr(binder);
            v.visit_pattern(pattern);
            v.visit_expr(body);
        }
    }
}
