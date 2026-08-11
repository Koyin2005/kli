use crate::{
    collect::CtxtRef,
    def_ids::DefId,
    diagnostics::DiagnosticReporter,
    patterns::{convert, pat::missing_patterns},
    src_loc::SrcLoc,
    typed_ast::{Expr, ExprKind, Pattern},
    typed_ast_visitor::{Visitor, walk_expr},
    types::Type,
};
pub struct PatternCheck<'ctxt> {
    diag: DiagnosticReporter,
    ctxt: CtxtRef<'ctxt>,
    id: DefId,
}
impl<'ctxt> PatternCheck<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>, id: DefId) -> Self {
        Self {
            diag: DiagnosticReporter::new(),
            ctxt,
            id,
        }
    }
    pub fn check(mut self, body: &Expr<'ctxt>) -> bool {
        self.visit_expr(body);
        self.diag.report_all()
    }
}
impl<'ctxt> Visitor<'ctxt> for PatternCheck<'ctxt> {
    fn visit_expr(&mut self, expr: &crate::typed_ast::Expr<'ctxt>) {
        match &expr.kind {
            ExprKind::Case(matchee, arms) => {
                self.visit_expr(matchee);
                check_patterns(
                    self.id,
                    self.ctxt,
                    &mut self.diag,
                    matchee.loc,
                    matchee.ty,
                    &arms.iter().map(|arm| &arm.pattern).collect::<Vec<_>>(),
                );
                for arm in arms {
                    self.visit_expr(&arm.body);
                }
            }
            _ => walk_expr(self, expr),
        }
    }
    fn visit_pattern(&mut self, pattern: &crate::typed_ast::Pattern<'ctxt>) {
        check_patterns(
            self.id,
            self.ctxt,
            &mut self.diag,
            pattern.loc,
            pattern.ty,
            &[pattern],
        );
    }
}

fn check_patterns<'ctxt>(
    id: DefId,
    ctxt: CtxtRef<'ctxt>,
    diag: &mut DiagnosticReporter,
    loc: SrcLoc,
    ty: Type<'ctxt>,
    patterns: &[&Pattern<'ctxt>],
) {
    let tys = [ty];
    let missing = missing_patterns(
        id,
        ctxt,
        &tys,
        &mut patterns
            .iter()
            .map(|pattern| convert::pattern_to_pat(ctxt, pattern)),
    );
    for pat in missing {
        diag.add_diagnostic(
            format!(
                "Missing pattern: {}",
                std::fmt::from_fn(|f| pat.format(ctxt, f))
            ),
            loc,
        );
    }
}
