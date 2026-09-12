use crate::{
    index_vec::IndexVec,
    mir::{Body, Local, PlaceBase, visitor::Visit},
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
        fn visit_assign(
            &mut self,
            _: crate::mir::Location,
            place: &crate::mir::Place,
            _: &crate::mir::Rvalue<'ctxt>,
        ) {
            let PlaceBase::Local(local) = place.base;
            if !place.projections.is_empty() {
                self.classes[local] = Some(_LocalClass::Memory);
                return;
            }
            let local_class = &mut self.classes[local];
            *local_class = match *local_class {
                Some(_LocalClass::Memory) => Some(_LocalClass::Memory),
                _ => Some(_LocalClass::Ssa),
            };
        }
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
