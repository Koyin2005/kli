use crate::layout::{self, Scalar, TagEncoding};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendRepr {
    Scalar(Scalar),
    ZeroSized,
    Memory,
}
pub fn backend_repr(layout: &layout::Layout) -> BackendRepr {
    if layout.is_zst() {
        return BackendRepr::ZeroSized;
    }
    match layout.kind {
        layout::LayoutKind::Aggregate(ref field_layouts, ..) => {
            let mut non_zst_fields = field_layouts.iter().filter(|field| !field.layout.is_zst());

            let Some(first_field) = non_zst_fields.next() else {
                return BackendRepr::Memory;
            };

            if non_zst_fields.next().is_none() {
                return backend_repr(&first_field.layout);
            }
            BackendRepr::Memory
        }
        layout::LayoutKind::Variant { tag, ref cases } => {
            let case_reprs = cases.iter().map(backend_repr).collect::<Vec<_>>();
            let tag_repr = match tag {
                TagEncoding::Uninhabited => BackendRepr::ZeroSized,
                TagEncoding::Field { scalar } => BackendRepr::Scalar(scalar),
            };
            if case_reprs
                .iter()
                .all(|case| matches!(case, BackendRepr::ZeroSized))
            {
                return tag_repr;
            }
            match case_reprs.as_slice() {
                [] => return tag_repr,
                [single] => return *single,
                _ => (),
            };
            BackendRepr::Memory
        }
        layout::LayoutKind::Scalar(scalar) => BackendRepr::Scalar(scalar),
        layout::LayoutKind::Uninit => BackendRepr::Memory,
    }
}
