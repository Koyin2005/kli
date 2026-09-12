use std::{cell::Cell, collections::HashSet};

use crate::{
    CtxtRef,
    ast::Mutable,
    resolved_ast::{Var, VarId},
    src_loc::SrcLoc,
    typed_ast::{ExprKind, Function, PatternKind, Place, PlaceKind},
    typed_ast_visitor::{Visitor, walk_expr, walk_pattern},
};

pub struct MutabilityCheck<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    mutable_variables: HashSet<VarId>,
    had_error: Cell<bool>,
}
impl<'ctxt> MutabilityCheck<'ctxt> {
    pub fn check(ctxt: CtxtRef<'ctxt>, function: &Function<'ctxt>) -> bool {
        let mut this = Self {
            ctxt,
            mutable_variables: HashSet::new(),
            had_error: Cell::new(false),
        };
        if let Some(body) = &function.body {
            this.visit_expr(body);
        }
        this.had_error.get()
    }
    fn add_mutable_var(&mut self, var: Var) {
        self.mutable_variables.insert(var.1);
    }

    fn modifiy_error(&self, var: Var, loc: SrcLoc) {
        self.ctxt
            .diag()
            .add_diagnostic(format!("Cannot modify variable '{}'", var.0), loc);
        self.had_error.set(true);
    }
    fn check_mutable(&self, place: &Place) {
        match place.kind {
            PlaceKind::Var(var) => {
                if !self.mutable_variables.contains(&var.1) {
                    self.modifiy_error(var, place.loc);
                }
            }
            PlaceKind::Upvar(.., var) => self.modifiy_error(var, place.loc),
            PlaceKind::Field(ref place, _) => {
                self.check_mutable(&place);
            }
            PlaceKind::Index(..) | PlaceKind::Deref(_) | PlaceKind::Invalid => (),
        }
    }
}
impl<'ctxt> Visitor<'ctxt> for MutabilityCheck<'ctxt> {
    fn visit_pattern(&mut self, pattern: &crate::typed_ast::Pattern<'ctxt>) {
        let PatternKind::Binding(mutable, var, _) = pattern.kind else {
            walk_pattern(self, pattern);
            return;
        };
        if matches!(mutable, Mutable::Mutable) {
            self.add_mutable_var(var);
        }
    }
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr<'ctxt>) {
        if let ExprKind::Assign(place, _) = &expr.kind {
            self.check_mutable(place);
        }
        walk_expr(self, expr);
    }
}
