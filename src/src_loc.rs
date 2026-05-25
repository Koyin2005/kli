use std::rc::Rc;
#[derive(Debug, Clone)]
pub struct SrcLoc {
    pub line: usize,
    pub file: Rc<str>,
}
impl SrcLoc {
    pub fn dummy() -> SrcLoc {
        SrcLoc {
            line: 0,
            file: Rc::from(""),
        }
    }
}
