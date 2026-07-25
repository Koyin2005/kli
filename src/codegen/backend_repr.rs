use crate::layout::{self, Scalar, TagEncoding};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendRepr {
    Scalar(Scalar),
    ScalarPair(Scalar, Scalar),
    ZeroSized,
}

pub fn backend_repr(layout: &layout::Layout) -> BackendRepr {
    if layout.is_zst() {
        return BackendRepr::ZeroSized;
    }
    match layout.kind {
        layout::LayoutKind::Aggregate(_) => BackendRepr::Scalar(Scalar::Pointer { non_null: true }),
        layout::LayoutKind::Variant {
            tag,
            data_offset,
            ref cases,
        } => {
            let case_reprs = cases.iter().map(backend_repr).collect::<Vec<_>>();
            let (tag_repr, tag_offset) = match tag {
                TagEncoding::Uninhabited => (BackendRepr::ZeroSized, layout::Size::ZERO),
                TagEncoding::Field { offset, scalar } => (BackendRepr::Scalar(scalar), offset),
            };
            if case_reprs
                .iter()
                .all(|case| matches!(case, BackendRepr::ZeroSized))
            {
                return tag_repr;
            }
            let (first, rest) = match case_reprs.as_slice() {
                [] => return tag_repr,
                [single] => return *single,
                [first, rest @ ..] => (first, rest),
            };
            if let BackendRepr::Scalar(scalar) = *first
                && let BackendRepr::Scalar(tag_repr) = tag_repr
                && rest.iter().all(|rest| rest == first)
            {
                return if tag_offset > data_offset {
                    BackendRepr::ScalarPair(scalar, tag_repr)
                } else {
                    BackendRepr::ScalarPair(tag_repr, scalar)
                };
            }

            BackendRepr::Scalar(Scalar::Pointer { non_null: true })
        }
        layout::LayoutKind::Scalar(scalar) => BackendRepr::Scalar(scalar),
        layout::LayoutKind::ScalarPair(scalar1, scalar2) => {
            BackendRepr::ScalarPair(scalar1, scalar2)
        }
    }
}
