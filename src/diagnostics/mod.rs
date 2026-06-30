use std::{cell::RefCell, fmt::Display};

use crate::src_loc::SrcLoc;

#[track_caller]
pub fn emit_fatal_diagnostic(loc: SrcLoc, msg: String) -> ! {
    panic!("{}", Diagnostic { loc, msg })
}
struct Diagnostic {
    msg: String,
    loc: SrcLoc,
}
impl Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.loc.line > 0 {
            write!(
                f,
                "Line [{}] in '{}': {}",
                self.loc.line, self.loc.file, self.msg
            )
        } else {
            write!(f, "Error : {}", self.msg)
        }
    }
}
#[derive(Default)]
pub struct DiagnosticReporter {
    diagnostics: RefCell<Vec<Diagnostic>>,
}
impl DiagnosticReporter {
    pub fn new() -> Self {
        Self {
            diagnostics: RefCell::new(Vec::new()),
        }
    }
    pub fn add_diagnostic(&self, msg: String, loc: SrcLoc) {
        self.diagnostics.borrow_mut().push(Diagnostic { msg, loc });
    }

    pub fn report_all(&self) -> bool {
        let mut emit_diagnostic = false;
        for diagnostic in self.diagnostics.borrow_mut().drain(..) {
            emit_diagnostic = true;
            eprintln!("{}", diagnostic);
        }
        emit_diagnostic
    }
}
