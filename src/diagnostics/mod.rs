struct Diagnostic {
    msg: String,
    line: usize,
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
    pub fn report(&mut self, msg: String, line: usize) {
        self.diagnostics.push(Diagnostic { msg, line });
    }

    pub fn finish(self) {
        for diagnostic in self.diagnostics {
            eprintln!("Line [{}] : {}", diagnostic.line, diagnostic.msg);
        }
    }
}
