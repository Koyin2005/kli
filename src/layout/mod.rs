use std::borrow::Cow;

use crate::{
    CtxtRef,
    ast::IsResource,
    collect::TypeDefKind,
    index_vec::IndexVec,
    typed_ast::FieldId,
    types::{CaseId, IntegerKind, Type},
};

pub const BITS_IN_BYTE: u8 = 8;
pub const POINTER_SIZE: Size = Size::BYTE.mul(8);
pub const POINTER_ALIGN: Align = Align::from_bytes(8).unwrap();

pub const INT_SIZE: Size = Size::BYTE.mul(8);
pub const INT_ALIGN: Align = Align::from_bytes(8).unwrap();

/// Size of an allocation in bytes
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct Size(u64);
impl Size {
    pub const ZERO: Self = Self(0);
    pub const BYTE: Self = Self(1);
    /// Largest possible size
    pub const MAX: Self = Self(i64::MAX as u64);

    pub const fn equal(self, other: Self) -> bool {
        self.0 == other.0
    }
    pub const fn from_bytes(bytes: u64) -> Option<Size> {
        if bytes > i64::MAX as u64 {
            return None;
        }
        Some(Self(bytes))
    }

    pub const fn from_bits(bits: u64) -> Option<Size> {
        let Some(bytes) = bits.checked_div(BITS_IN_BYTE as u64) else {
            return None;
        };
        Self::from_bytes(bytes)
    }

    pub const fn add(self, other: Self) -> Self {
        Self(self.0.strict_add(other.0))
    }

    pub const fn mul(self, other: u64) -> Self {
        self.checked_mul(other).expect("too big")
    }

    pub const fn align_to(self, align: Align) -> Self {
        Self(self.0.next_multiple_of(align.in_bytes()))
    }

    pub const fn checked_mul(self, other: u64) -> Option<Self> {
        let Some(value) = self.0.checked_mul(other) else {
            return None;
        };
        Some(Self(value))
    }

    pub const fn in_bytes(self) -> u64 {
        self.0
    }

    #[track_caller]
    pub const fn in_bits(self) -> u64 {
        self.0
            .checked_mul(BITS_IN_BYTE as u64)
            .expect("too big for bits")
    }
}

#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash, Debug)]
pub struct Align(u8);
impl Align {
    pub const BYTE: Self = Self(0);
    pub const FOUR_BYTE: Self = Self(2);

    pub const fn from_bytes(alignment: u64) -> Option<Align> {
        let Some(pow_2) = alignment.checked_ilog2() else {
            return None;
        };
        Some(Align(pow_2 as u8))
    }
    pub const fn in_bytes(self) -> u64 {
        2u64.pow(self.0 as u32)
    }
}
#[derive(Clone, Copy, Debug)]
pub enum TagEncoding {
    Uninhabited,
    Field { offset: Size, scalar: Scalar },
    Data { offset: Size },
}
#[derive(Clone, Debug)]
pub struct VariantLayout {
    pub field: FieldLayout,
}
#[derive(Clone, Debug)]
pub struct FieldLayout {
    pub offset: Size,
    pub layout: Layout,
}
#[derive(Clone, Debug)]
pub struct Layout {
    pub size: Size,
    pub alignment: Align,
    pub kind: LayoutKind,
}
impl Layout {
    pub const BYTE: Self = Self {
        size: Size::BYTE,
        alignment: Align::BYTE,
        kind: LayoutKind::Scalar(Scalar::Byte),
    };
    pub const fn pointer(non_null: bool) -> Self {
        Self {
            size: POINTER_SIZE,
            alignment: POINTER_ALIGN,
            kind: LayoutKind::Scalar(Scalar::Pointer { non_null }),
        }
    }
    pub const fn as_scalar(&self) -> Option<Scalar> {
        let LayoutKind::Scalar(scalar) = self.kind else {
            return None;
        };
        Some(scalar)
    }
    pub const fn is_uninhabited(&self) -> bool {
        let LayoutKind::Variant { tag, .. } = self.kind else {
            return false;
        };
        matches!(tag, TagEncoding::Uninhabited)
    }
    pub const fn zst() -> Self {
        Self {
            size: Size::ZERO,
            alignment: Align::BYTE,
            kind: LayoutKind::Aggregate(IndexVec::new()),
        }
    }
    pub const fn uninhabited(&self) -> Self {
        Self {
            size: self.size,
            alignment: self.alignment,
            kind: LayoutKind::Variant {
                tag: TagEncoding::Uninhabited,
                data_offset: Size::ZERO,
                cases: IndexVec::new(),
            },
        }
    }
    pub const fn is_zst(&self) -> bool {
        self.size.equal(Size::ZERO)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scalar {
    Byte,
    Bool,
    Pointer { non_null: bool },
    Uint32,
    Int64(IntegerKind),
}
#[derive(Clone, Debug)]
pub enum LayoutKind {
    Aggregate(IndexVec<FieldId, FieldLayout>),
    Variant {
        tag: TagEncoding,
        data_offset: Size,
        cases: IndexVec<CaseId, Layout>,
    },
    Scalar(Scalar),
}

pub enum LayoutError {
    TooGeneric,
    Unknown,
    TooBig,
}

fn variant_layout(
    ctxt: CtxtRef<'_>,
    cases: IndexVec<CaseId, Option<Type>>,
) -> Result<Layout, LayoutError> {
    if cases.is_empty() {
        return Ok(Layout::zst().uninhabited());
    }
    let case_layouts = cases
        .iter()
        .map(|case| {
            if let Some(case) = case {
                calculate_layout(ctxt, case)
            } else {
                Ok(Layout::zst())
            }
        })
        .collect::<Result<IndexVec<CaseId, _>, _>>()?;
    let (tag_size, tag_scalar, tag_align) = if cases.len() < 256usize {
        (Size::BYTE, Scalar::Byte, Align::BYTE)
    } else {
        (INT_SIZE, Scalar::Int64(IntegerKind::Unsigned), INT_ALIGN)
    };

    let biggest_size = case_layouts
        .iter()
        .reduce(
            |acc, layout| {
                if acc.size >= layout.size { acc } else { layout }
            },
        )
        .unwrap();

    let (tag_offset, data_offset, max_align) = if tag_align < biggest_size.alignment {
        /* tag data */
        (Size::ZERO, tag_size, biggest_size.alignment)
    } else {
        /*data tag */
        (biggest_size.size, Size::ZERO, tag_align)
    };

    Ok(Layout {
        size: tag_size.add(biggest_size.size).align_to(max_align),
        alignment: max_align,
        kind: LayoutKind::Variant {
            tag: TagEncoding::Field {
                offset: tag_offset,
                scalar: tag_scalar,
            },
            data_offset,
            cases: case_layouts,
        },
    })
}

fn aggregate_layout(mut field_layouts: Vec<(FieldId, Layout)>) -> Result<Layout, LayoutError> {
    field_layouts.sort_by_key(|(_, layout)| layout.alignment);

    let mut offset = Size::ZERO;
    let min_align = field_layouts[0].1.alignment;
    let mut layouts = IndexVec::<FieldId, _>::from_value(
        field_layouts.len(),
        FieldLayout {
            offset,
            layout: Layout::zst(),
        },
    );
    for (field, layout) in field_layouts {
        let align = layout.alignment;
        let size = layout.size;
        layouts[field] = FieldLayout { offset, layout };
        offset = offset.add(size).align_to(align);
    }
    Ok(Layout {
        size: offset,
        alignment: min_align,
        kind: LayoutKind::Aggregate(layouts),
    })
}
fn record_layout(
    ctxt: CtxtRef<'_>,
    fields: IndexVec<FieldId, Cow<'_, Type>>,
) -> Result<Layout, LayoutError> {
    if fields.is_empty() {
        return Ok(Layout::zst());
    }
    if let Some([field]) = fields.as_slice().as_array() {
        return calculate_layout(ctxt, field);
    }
    let field_layouts = fields
        .iter_enumerated()
        .map(|(i, field)| Ok((i, calculate_layout(ctxt, field)?)))
        .collect::<Result<Vec<_>, _>>()?;
    aggregate_layout(field_layouts)
}
pub fn calculate_layout(ctxt: CtxtRef<'_>, ty: &Type) -> Result<Layout, LayoutError> {
    Ok(match ty {
        Type::Infer(_) | Type::Unknown => return Err(LayoutError::Unknown),
        Type::Int(integer_kind) => Layout {
            size: INT_SIZE,
            alignment: INT_ALIGN,
            kind: LayoutKind::Scalar(Scalar::Int64(*integer_kind)),
        },
        Type::Bool => Layout {
            size: Size::BYTE,
            alignment: Align::BYTE,
            kind: LayoutKind::Scalar(Scalar::Bool),
        },
        Type::Char => Layout {
            size: Size::BYTE.mul(4),
            alignment: Align::FOUR_BYTE,
            kind: LayoutKind::Scalar(Scalar::Uint32),
        },
        Type::Byte => Layout::BYTE,
        Type::Never => Layout::zst().uninhabited(),
        Type::Param(_, _) => return Err(LayoutError::TooGeneric),
        Type::Function(function_type) => match function_type.resource {
            IsResource::Data => Layout::pointer(true),
            IsResource::Resource => {
                return record_layout(
                    ctxt,
                    IndexVec::from_vec(vec![
                        Cow::Owned(Type::pointer(Type::Byte)),
                        Cow::Owned(Type::new_function(
                            function_type.params.clone(),
                            (*function_type.return_type).clone(),
                        )),
                    ]),
                );
            }
        },
        Type::Tuple(fields) => {
            return record_layout(ctxt, fields.iter().map(Cow::Borrowed).collect());
        }
        Type::Record(fields) => {
            return record_layout(
                ctxt,
                fields
                    .into_iter()
                    .map(|field| Cow::Borrowed(&field.ty))
                    .collect(),
            );
        }
        Type::RawPointer(_) | Type::Imm(..) | Type::Mut(..) => {
            Layout::pointer(!matches!(ty, Type::RawPointer(_)))
        }
        Type::Array(ty, count) => {
            let mut element_layout = calculate_layout(ctxt, ty)?;
            element_layout.size = element_layout
                .size
                .checked_mul(*count)
                .ok_or(LayoutError::TooBig)?;
            element_layout
        }
        Type::Named(id, .., args) => match ctxt.type_def(*id).kind {
            TypeDefKind::Record(fields) => {
                return record_layout(
                    ctxt,
                    fields
                        .into_iter()
                        .map(|field| Cow::Owned(field.type_of(args, ctxt)))
                        .collect(),
                );
            }
            TypeDefKind::Variant(cases) => {
                return variant_layout(
                    ctxt,
                    cases
                        .into_iter()
                        .map(|case| case.field.map(|field| field.type_of(args, ctxt)))
                        .collect(),
                );
            }
        },
    })
}
