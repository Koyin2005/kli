use crate::{index_vec::IndexVec, ir};

pub mod vm;

pub enum AssignCount {
    None,
    Single,
    Many,
}

pub fn classify_locals(body: &ir::Body) -> IndexVec<ir::Local, AssignCount> {
    let mut kinds: IndexVec<ir::Local, AssignCount> = body
        .locals
        .indices()
        .map(|i| {
            if i.into_u32() < body.param_count {
                AssignCount::Single
            } else {
                AssignCount::None
            }
        })
        .collect();
    fn visit_place(
        kinds: &mut IndexVec<ir::Local, AssignCount>,
        body: &ir::Body,
        place: &ir::Place,
    ) {
        match place {
            ir::Place::Local(_) => (),
            ir::Place::Deref(place)
            | ir::Place::Downcast(place, _)
            | ir::Place::Field(place, _) => visit_place(kinds, body, place),
            ir::Place::Index(place, value) => {
                visit_place(kinds, body, place);
                visit_expr(kinds, body, value);
            }
        }
    }
    fn visit_expr(kinds: &mut IndexVec<ir::Local, AssignCount>, body: &ir::Body, expr: &ir::Expr) {
        match &expr.kind {
            ir::ExprKind::Constant(_) => (),
            ir::ExprKind::Load(place) => visit_place(kinds, body, place),
            ir::ExprKind::Len(array) => visit_expr(kinds, body, array),
            ir::ExprKind::Discriminant(place) => visit_place(kinds, body, place),
            ir::ExprKind::Aggregate(_, fields) => {
                for field in fields {
                    visit_expr(kinds, body, field);
                }
            }
            ir::ExprKind::BinaryOp(_, left, right) => {
                visit_expr(kinds, body, left);
                visit_expr(kinds, body, right);
            }
            ir::ExprKind::Not(expr) => visit_expr(kinds, body, expr),
        }
    }
    fn visit_stmt(kinds: &mut IndexVec<ir::Local, AssignCount>, body: &ir::Body, stmt: &ir::Stmt) {
        match stmt {
            ir::Stmt::Panic => (),
            ir::Stmt::PanicIf(expr) => visit_expr(kinds, body, expr),
            ir::Stmt::Print { value, is_err: _ } => visit_expr(kinds, body, value),
            ir::Stmt::Loop(_, stmts) => {
                for stmt in stmts {
                    visit_stmt(kinds, body, stmt);
                }
            }
            ir::Stmt::Break(_) => (),
            ir::Stmt::If(condition, then_branch, else_branch) => {
                visit_expr(kinds, body, condition);
                for stmt in then_branch {
                    visit_stmt(kinds, body, stmt);
                }
                for stmt in else_branch {
                    visit_stmt(kinds, body, stmt);
                }
            }
            ir::Stmt::Match(_) => todo!("idk bout matches"),
            ir::Stmt::Call(call) => {
                visit_expr(kinds, body, &call.callee);
                for arg in &call.args {
                    visit_expr(kinds, body, arg);
                }
                visit_place_assign(kinds, body, &call.return_place);
            }
            ir::Stmt::ReadLine(place) => {
                visit_place_assign(kinds, body, place);
            }
            ir::Stmt::Alloc(place, allocate) => {
                match allocate {
                    ir::Allocate::Array(_, elements) => {
                        for element in elements {
                            visit_expr(kinds, body, element);
                        }
                    }
                }
                visit_place_assign(kinds, body, place);
            }
            ir::Stmt::Assign(place, expr) => {
                visit_expr(kinds, body, expr);
                visit_place_assign(kinds, body, place);
            }
            ir::Stmt::Return(expr) => {
                visit_expr(kinds, body, expr);
            }
        }
    }
    fn visit_place_assign(
        kinds: &mut IndexVec<ir::Local, AssignCount>,
        body: &ir::Body,
        place: &ir::Place,
    ) {
        match place {
            ir::Place::Local(local) => match &mut kinds[*local] {
                kind @ AssignCount::None => *kind = AssignCount::Single,
                AssignCount::Many => (),
                kind @ AssignCount::Single => *kind = AssignCount::Many,
            },
            ir::Place::Field(place, _) => {
                visit_place_assign(kinds, body, place);
            }
            ir::Place::Deref(_) => (),
            ir::Place::Downcast(place, _) => {
                visit_place_assign(kinds, body, place);
            }
            ir::Place::Index(_, expr) => {
                visit_expr(kinds, body, expr);
            }
        }
    }
    for stmt in body.body.iter() {
        visit_stmt(&mut kinds, body, stmt);
    }
    kinds
}
