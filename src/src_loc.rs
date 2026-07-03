use crate::ident::Symbol;
#[derive(Debug, Clone, Copy)]
pub struct SrcLoc {
    pub line: u32,
    pub file: Symbol,
}
impl SrcLoc {
    pub const fn dummy() -> SrcLoc {
        SrcLoc {
            line: 0,
            file: Symbol::EMPTY_STRING,
        }
    }
    pub const fn with_file(self, file: Symbol) -> Self {
        Self { file, ..self }
    }
}
