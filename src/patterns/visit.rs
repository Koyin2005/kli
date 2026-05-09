use crate::{
    diagnostics::DiagnosticReporter,
    patterns::{convert, pat::missing_patterns},
    typed_ast::{Expr, ExprKind, Pattern},
    typed_ast_visitor::{Visitor, walk_expr},
    types::Type,
};
#[derive(Default)]
pub struct PatternCheck {
    diag: DiagnosticReporter,
}
impl PatternCheck {
    pub fn new() -> Self {
        Self {
            diag: DiagnosticReporter::new(),
        }
    }
    pub fn check(mut self, body: &Expr) {
        self.visit_expr(body);
        self.diag.finish();
    }
}
impl Visitor for PatternCheck {
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr) {
        match &expr.kind {
            ExprKind::Case(matchee, arms) => {
                self.visit_expr(matchee);
                check_patterns(
                    &mut self.diag,
                    matchee.line,
                    &matchee.ty,
                    &arms.iter().map(|arm| &arm.pattern).collect::<Vec<_>>(),
                );
                for arm in arms {
                    self.visit_expr(&arm.body);
                }
            }
            _ => walk_expr(self, expr),
        }
    }
    fn visit_pattern(&mut self, pattern: &crate::typed_ast::Pattern) {
        check_patterns(&mut self.diag, pattern.line, &pattern.ty, &[pattern]);
    }
}

fn check_patterns(diag: &mut DiagnosticReporter, line: usize, ty: &Type, patterns: &[&Pattern]) {
    let missing = missing_patterns(
        ty,
        &mut patterns
            .iter()
            .map(|pattern| convert::pattern_to_pat(pattern)),
    );
    for pat in missing {
        diag.report(format!("Missing pattern: {}", pat), line);
    }
}
