use crate::layout::{self, Scalar, Size, TagEncoding};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BackendRepr {
    Scalar(Scalar),
    ScalarPair {
        first: Scalar,
        second: Scalar,
        second_offset: Size,
    },
    ZeroSized,
    Memory,
}

pub fn backend_repr(layout: &layout::Layout) -> BackendRepr {
    if layout.is_zst() {
        return BackendRepr::ZeroSized;
    }
    match layout.kind {
        layout::LayoutKind::Aggregate(ref field_layouts, ref field_offsets) => {
            let non_zst_fields = field_layouts
                .iter()
                .filter(|field| !field.layout.is_zst())
                .collect::<Vec<_>>();
            match non_zst_fields.as_slice() {
                [first_field, second_field]
                    if let Some(first) = first_field.layout.as_scalar()
                        && let Some(second) = second_field.layout.as_scalar()
                        && let second_offset = field_offsets[second_field.field].offset =>
                {
                    return BackendRepr::ScalarPair {
                        first,
                        second,
                        second_offset,
                    };
                }
                [single] => return backend_repr(&single.layout),
                _ => (),
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
            if let BackendRepr::Scalar(tag_repr) = tag_repr {
                let mut biggest = None::<Scalar>;
                for &repr in &case_reprs {
                    let scalar = match repr {
                        BackendRepr::Memory | BackendRepr::ScalarPair { .. } => {
                            return BackendRepr::Memory;
                        }
                        BackendRepr::Scalar(single) => single,
                        BackendRepr::ZeroSized => continue,
                    };
                    let curr_size = if let Some(biggest) = biggest {
                        biggest.size()
                    } else {
                        Size::ZERO
                    };
                    if curr_size < scalar.size() {
                        biggest = Some(scalar);
                    }
                }
                let (first, second, second_offset) = (tag_repr, biggest.unwrap(), tag_repr.size());

                return BackendRepr::ScalarPair {
                    first,
                    second,
                    second_offset,
                };
            }

            BackendRepr::Memory
        }
        layout::LayoutKind::Scalar(scalar) => BackendRepr::Scalar(scalar),
        layout::LayoutKind::ScalarPair(first, second, second_offset) => BackendRepr::ScalarPair {
            first,
            second,
            second_offset,
        },
    }
}
