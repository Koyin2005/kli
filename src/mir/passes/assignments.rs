use crate::{
    index_vec::IndexVec,
    mir::{Body, Local, visitor::Visit},
};

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum _LocalClass {
    Memory,
    Ssa,
}

pub fn _classify_locals<'ctxt>(body: &Body<'ctxt>) -> IndexVec<Local, _LocalClass> {
    struct LocalClassifier<'a> {
        classes: &'a mut IndexVec<Local, Option<_LocalClass>>,
    }
    impl<'ctxt> Visit<'ctxt> for LocalClassifier<'_> {
    }

    let mut local_classes =
        IndexVec::<Local, _>::from_value(body.locals.len(), None::<_LocalClass>);
    LocalClassifier {
        classes: &mut local_classes,
    }
    .visit_body(body);
    local_classes
        .into_iter()
        .map(|class| {
            if let Some(class) = class {
                class
            } else {
                _LocalClass::Ssa
            }
        })
        .collect()
}
