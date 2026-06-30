use crate::ident::Symbol;
#[derive(Debug, Clone, Copy)]
pub struct SrcLoc {
    pub line: usize,
    pub file: Symbol,
}
impl SrcLoc {
    pub fn dummy() -> SrcLoc {
        SrcLoc {
            line: 0,
            file: Symbol::EMPTY_STRING,
        }
    }
}
