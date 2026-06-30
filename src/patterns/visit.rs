use crate::{
    diagnostics::DiagnosticReporter,
    patterns::{convert, pat::missing_patterns},
    src_loc::SrcLoc,
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
    pub fn check(mut self, body: &Expr) -> bool {
        self.visit_expr(body);
        self.diag.report_all()
    }
}
impl Visitor for PatternCheck {
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr) {
        match &expr.kind {
            ExprKind::Case(matchee, arms) => {
                self.visit_expr(matchee);
                check_patterns(
                    &mut self.diag,
                    matchee.loc,
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
        check_patterns(&mut self.diag, pattern.loc, &pattern.ty, &[pattern]);
    }
}

fn check_patterns(diag: &mut DiagnosticReporter, loc: SrcLoc, ty: &Type, patterns: &[&Pattern]) {
    let tys = [ty];
    let missing = missing_patterns(
        &tys,
        &mut patterns
            .iter()
            .map(|pattern| convert::pattern_to_pat(pattern)),
    );
    for pat in missing {
        diag.add_diagnostic(format!("Missing pattern: {}", pat), loc);
    }
}
