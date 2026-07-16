use crate::{
    mir::build::Builder,
    typed_ast::{Expr, IteratorType, Pattern},
};

impl Builder<'_> {
    pub(super) fn for_loop(
        &mut self,
        _: &Pattern,
        _: &Expr,
        iterator_type: &IteratorType,
        _: &Expr,
    ) {
        match *iterator_type {}
    }
}
