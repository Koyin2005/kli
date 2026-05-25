use crate::src_loc::SrcLoc;

struct Diagnostic {
    msg: String,
    loc: SrcLoc,
}
#[derive(Default)]
pub struct DiagnosticReporter {
    diagnostics: Vec<Diagnostic>,
}
impl DiagnosticReporter {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
        }
    }
    pub fn report(&mut self, msg: String, loc: SrcLoc) {
        self.diagnostics.push(Diagnostic { msg, loc });
    }

    pub fn finish(self) -> bool {
        let mut emit_diagnostic = false;
        for diagnostic in self.diagnostics {
            emit_diagnostic = true;
            if diagnostic.loc.line > 0 {
                eprintln!(
                    "Line [{}] in '{}': {}",
                    diagnostic.loc.line, diagnostic.loc.file, diagnostic.msg
                );
            } else {
                eprintln!("Error : {}", diagnostic.msg);
            }
        }
        emit_diagnostic
    }
}
