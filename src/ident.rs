use std::rc::Rc;

use crate::src_loc::SrcLoc;

#[derive(Debug, Clone)]
pub struct Ident {
    pub content: Rc<str>,
    pub loc: SrcLoc,
}
