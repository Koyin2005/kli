use std::cell::RefCell;

use crate::src_loc::SrcLoc;

#[track_caller]
pub fn emit_fatal_diagnostic(loc: SrcLoc, msg: String) -> ! {
    if loc.line > 0 {
        panic!("Line [{}] in '{}': {}", loc.line, loc.file, msg);
    } else {
        panic!("Error : {}", msg);
    }
}
struct Diagnostic {
    msg: String,
    loc: SrcLoc,
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
    fn emit_diagnostic(diagnostic: Diagnostic) {
        if diagnostic.loc.line > 0 {
            eprintln!(
                "Line [{}] in '{}': {}",
                diagnostic.loc.line, diagnostic.loc.file, diagnostic.msg
            );
        } else {
            eprintln!("Error : {}", diagnostic.msg);
        }
    }
    pub fn add_diagnostic(&self, msg: String, loc: SrcLoc) {
        self.diagnostics.borrow_mut().push(Diagnostic { msg, loc });
    }

    pub fn report_all(&self) -> bool {
        let mut emit_diagnostic = false;
        for diagnostic in self.diagnostics.borrow_mut().drain(..) {
            emit_diagnostic = true;
            Self::emit_diagnostic(diagnostic);
        }
        emit_diagnostic
    }
}
