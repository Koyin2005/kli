use crate::{
    src_loc::SrcLoc,
    typed_ast::{Expr, ExprKind, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind},
    types::Type,
};

pub trait Visitor<'ctxt> {
    fn visit_lit(&mut self, loc: SrcLoc, lit: u64, ty: Type<'ctxt>) {
        _ = loc;
        _ = lit;
        _ = ty;
    }
    fn visit_expr(&mut self, expr: &Expr<'ctxt>) {
        walk_expr(self, expr);
    }
    fn visit_place(&mut self, place: &Place<'ctxt>) {
        walk_place(self, place);
    }
    fn visit_pattern(&mut self, pattern: &Pattern<'ctxt>) {
        walk_pattern(self, pattern);
    }
    fn visit_stmt(&mut self, stmt: &Stmt<'ctxt>) {
        walk_stmt(self, stmt);
    }
}
pub fn walk_pattern<'ctxt, V>(v: &mut V, pattern: &Pattern<'ctxt>)
where
    V: Visitor<'ctxt> + ?Sized,
{
    match &pattern.kind {
        PatternKind::Int(value) => {
            v.visit_lit(pattern.loc, *value, pattern.ty);
        }
        PatternKind::Binding(..) | PatternKind::Err | PatternKind::Bool(_) | PatternKind::Unit => {
            ()
        }
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
pub fn walk_place<'ctxt, V>(v: &mut V, place: &Place<'ctxt>)
where
    V: Visitor<'ctxt> + ?Sized,
{
    match &place.kind {
        PlaceKind::Var(_) | PlaceKind::Upvar(..) | PlaceKind::Invalid => (),
        PlaceKind::Deref(expr) => {
            v.visit_expr(expr);
        }
        PlaceKind::Field(place, _) => v.visit_place(place),
        PlaceKind::Index(expr1, expr2) => {
            v.visit_expr(expr1);
            v.visit_expr(expr2);
        }
    }
}
pub fn walk_stmt<'ctxt, V>(v: &mut V, stmt: &Stmt<'ctxt>)
where
    V: Visitor<'ctxt> + ?Sized,
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
pub fn walk_expr<'ctxt, V>(v: &mut V, expr: &Expr<'ctxt>)
where
    V: Visitor<'ctxt> + ?Sized,
{
    match &expr.kind {
        ExprKind::Int(value) => v.visit_lit(expr.loc, *value, expr.ty),
        ExprKind::Block(body) => {
            for stmt in &body.stmts {
                v.visit_stmt(stmt);
            }
            v.visit_expr(&body.expr);
        }
        ExprKind::NamedRecord(.., fields) => {
            for field in fields {
                v.visit_expr(&field.value);
            }
        }
        ExprKind::Err
        | ExprKind::Char(_)
        | ExprKind::Const(..)
        | ExprKind::Bool(_)
        | ExprKind::String(_)
        | ExprKind::Function(..)
        | ExprKind::Unit
        | ExprKind::Panic => (),
        ExprKind::BuiltinCall(.., exprs) | ExprKind::Tuple(exprs) | ExprKind::Array(exprs) => {
            for expr in exprs {
                v.visit_expr(expr);
            }
        }
        ExprKind::NeverToAny(value) | ExprKind::Unsafe(value) | ExprKind::Return(value) => {
            v.visit_expr(value)
        }
        ExprKind::VariantInit(.., value) => {
            if let Some(value) = value {
                v.visit_expr(value)
            }
        }
        ExprKind::Call(callee, args) => {
            v.visit_expr(callee);
            args.iter().for_each(|expr| v.visit_expr(expr));
        }
        ExprKind::Binary(_, first, second)
        | ExprKind::While(first, second)
        | ExprKind::Logic(_, first, second) => {
            v.visit_expr(first);
            v.visit_expr(second)
        }
        ExprKind::Load(place) => v.visit_place(place),
        ExprKind::Assign(place, value) => {
            v.visit_place(place);
            v.visit_expr(value);
        }
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
