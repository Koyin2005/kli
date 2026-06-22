use crate::{
    diagnostics::DiagnosticReporter,
    mir::{Body, Context, Stmt, visitor::Visit},
    src_loc::SrcLoc,
};
pub const CHECK_WELL_FORMED: bool = false;
pub struct WellFormed<'ctxt> {
    ctxt: &'ctxt Context,
    body: &'ctxt Body,
    diag: &'ctxt mut DiagnosticReporter,
}
impl<'ctxt> WellFormed<'ctxt> {
    pub fn new(
        body: &'ctxt Body,
        ctxt: &'ctxt Context,
        diag: &'ctxt mut DiagnosticReporter,
    ) -> Self {
        Self { ctxt, diag, body }
    }
    fn assert(&mut self, condition: bool, msg: impl FnOnce() -> String, loc: SrcLoc) {
        if condition {
            self.diag.add_diagnostic(msg(), loc);
        }
    }
}
impl Visit for WellFormed<'_> {}
