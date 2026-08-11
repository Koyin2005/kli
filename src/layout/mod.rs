use std::cmp::max;

use crate::{
    CtxtRef,
    collect::TypeDefKind,
    index_vec::IndexVec,
    typed_ast::FieldId,
    types::{CaseId, IntegerSize, TagType, Type, TypeKind},
};

pub const BITS_IN_BYTE: u8 = 8;
pub const POINTER_SIZE: Size = Size::BYTE.mul(8);
pub const POINTER_ALIGN: Align = Align::from_bytes(8).unwrap();

pub const INT_SIZE: Size = Size::BYTE.mul(8);
pub const INT_ALIGN: Align = Align::from_bytes(8).unwrap();

/// Size of an allocation in bytes
#[derive(PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Hash, Debug, Default)]
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
        if self.0.is_multiple_of(align.in_bytes()) {
            return self;
        }
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
    pub const fn in_bytes_u32(self) -> u32 {
        if self.0 < u32::MAX as u64 {
            self.0 as _
        } else {
            panic!("expected this value to be less than u32::MAX")
        }
    }
    #[track_caller]
    pub const fn in_bytes_i32(self) -> i32 {
        if self.0 < i32::MAX as u64 {
            self.0 as i32
        } else {
            panic!("expected this value to be less than i32::MAX")
        }
    }
    #[track_caller]
    pub const fn in_bytes_i64(self) -> i64 {
        if self.0 < i64::MAX as u64 {
            self.0 as _
        } else {
            panic!("expected this value to be less than i64::MAX")
        }
    }
    #[track_caller]
    pub const fn in_bytes_usize(self) -> usize {
        if self.0 < usize::MAX as u64 {
            self.0 as _
        } else {
            panic!("expected this value to be less than usize::MAX")
        }
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
    pub const fn pow_of_2(self) -> u8 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagEncoding {
    Uninhabited,
    Field { scalar: Scalar },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariantLayout {
    pub field: FieldLayout,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLayout {
    pub field: FieldId,
    pub layout: Layout,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub size: Size,
    pub alignment: Align,
    pub kind: LayoutKind,
}
impl Layout {
    pub const BYTE: Self = Self {
        size: Size::BYTE,
        alignment: Align::BYTE,
        kind: LayoutKind::Scalar(Scalar::Int {
            signed: false,
            size: IntegerSize::Int8,
        }),
    };
    ///Produces an aggregate with only layout as its field, but
    /// is prefixed by prefix size
    pub fn prefixed_by(prefix: Size, align: Align, layout: Self) -> Self {
        let align = align.max(layout.alignment);
        let total_size = prefix.add(layout.size).align_to(align);
        let offset = prefix.align_to(align);
        Self {
            size: total_size,
            alignment: layout.alignment,
            kind: LayoutKind::Aggregate(
                vec![FieldLayout {
                    field: FieldId::FIRST_FIELD,
                    layout,
                }],
                IndexVec::from_vec(vec![FieldOffset {
                    index_in_memory: 0,
                    offset,
                }]),
            ),
        }
    }
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
    pub const fn from_scalar(scalar: Scalar) -> Self {
        Self {
            size: scalar.size(),
            alignment: scalar.align(),
            kind: LayoutKind::Scalar(scalar),
        }
    }
    pub const fn zst() -> Self {
        Self {
            size: Size::ZERO,
            alignment: Align::BYTE,
            kind: LayoutKind::Aggregate(Vec::new(), IndexVec::new()),
        }
    }
    pub const fn uninhabited(&self) -> Self {
        Self {
            size: self.size,
            alignment: self.alignment,
            kind: LayoutKind::Variant {
                tag: TagEncoding::Uninhabited,
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
    Bool,
    Pointer { non_null: bool },
    Int { signed: bool, size: IntegerSize },
}
impl Scalar {
    pub const BYTE: Self = Self::uint(IntegerSize::Int8);

    pub const fn integer(signed: bool, size: IntegerSize) -> Self {
        Self::Int { signed, size }
    }
    pub const fn uint(size: IntegerSize) -> Self {
        Self::integer(false, size)
    }
    pub const fn size(self) -> Size {
        match self {
            Scalar::Bool => Size::BYTE,
            Scalar::Pointer { non_null: _ } => POINTER_SIZE,
            Scalar::Int { size, .. } => match size {
                IntegerSize::Int64 => Size::BYTE.mul(8),
                IntegerSize::Int32 => Size::BYTE.mul(4),
                IntegerSize::Int8 => Size::BYTE,
            },
        }
    }
    pub const fn align(self) -> Align {
        match self {
            Self::Bool => Align::BYTE,
            Self::Int { size, .. } => match size {
                IntegerSize::Int8 => Align::BYTE,
                IntegerSize::Int32 => Align::FOUR_BYTE,
                IntegerSize::Int64 => INT_ALIGN,
            },
            Self::Pointer { non_null: _ } => POINTER_ALIGN,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FieldOffset {
    pub index_in_memory: usize,
    pub offset: Size,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutKind {
    Aggregate(Vec<FieldLayout>, IndexVec<FieldId, FieldOffset>),
    Variant {
        tag: TagEncoding,
        cases: IndexVec<CaseId, Layout>,
    },
    Scalar(Scalar),
    Uninit(Box<Layout>),
}

#[derive(Clone)]
pub enum LayoutError {
    TooGeneric,
    Unknown,
    TooBig,
}
impl std::fmt::Debug for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooBig => write!(f, "too big"),
            Self::Unknown => write!(f, "could not compute unknown layout"),
            Self::TooGeneric => write!(f, "do not know the layout of generic type"),
        }
    }
}

fn variant_layout<'ctxt>(
    ctxt: CtxtRef<'ctxt>,
    tag_type: TagType,
    cases: IndexVec<CaseId, Option<Type<'ctxt>>>,
) -> Result<Layout, LayoutError> {
    if cases.is_empty() {
        return Ok(Layout::zst().uninhabited());
    }
    let case_layouts = cases
        .iter()
        .map(|&case| {
            if let Some(case) = case {
                calculate_layout(ctxt, case)
            } else {
                Ok(Layout::zst())
            }
        })
        .collect::<Result<IndexVec<CaseId, _>, _>>()?;
    let (tag_size, tag_scalar, tag_align) = if let TagType::UInt8 | TagType::Never = tag_type {
        (Size::BYTE, Scalar::BYTE, Align::BYTE)
    } else {
        (
            INT_SIZE,
            Scalar::integer(false, IntegerSize::Int64),
            INT_ALIGN,
        )
    };

    let biggest_size = case_layouts
        .iter()
        .reduce(
            |acc, layout| {
                if acc.size >= layout.size { acc } else { layout }
            },
        )
        .unwrap();

    let max_align = tag_align.max(biggest_size.alignment);

    Ok(Layout {
        size: tag_size.add(biggest_size.size).align_to(max_align),
        alignment: max_align,
        kind: LayoutKind::Variant {
            tag: TagEncoding::Field { scalar: tag_scalar },
            cases: case_layouts,
        },
    })
}

fn aggregate_layout(field_layouts: Vec<(FieldId, Layout)>) -> Result<Layout, LayoutError> {
    let mut offset = Size::ZERO;
    let mut max_align = field_layouts
        .first()
        .map_or(Align::BYTE, |(_, layout)| layout.alignment);
    let mut field_positions = IndexVec::from_value(field_layouts.len(), FieldOffset::default());
    let layouts = field_layouts
        .into_iter()
        .enumerate()
        .map(|(i, (field, layout))| {
            let current_align = layout.alignment;
            max_align = max(current_align, max_align);
            let size = layout.size;
            offset = offset.align_to(max_align);
            field_positions[field] = FieldOffset {
                index_in_memory: i,
                offset,
            };
            let layout = FieldLayout { field, layout };
            offset = offset.add(size).align_to(max_align);
            layout
        })
        .collect();
    Ok(Layout {
        size: offset.align_to(max_align),
        alignment: max_align,
        kind: LayoutKind::Aggregate(layouts, field_positions),
    })
}
fn record_layout<'ctxt>(
    ctxt: CtxtRef<'ctxt>,
    fields: IndexVec<FieldId, Type<'ctxt>>,
) -> Result<Layout, LayoutError> {
    let mut field_layouts = fields
        .iter_enumerated()
        .map(|(i, field)| Ok((i, calculate_layout(ctxt, *field)?)))
        .collect::<Result<Vec<_>, _>>()?;

    field_layouts.sort_by_key(|(_, layout)| std::cmp::Reverse(layout.alignment));
    aggregate_layout(field_layouts)
}
pub fn calculate_layout<'ctxt>(
    ctxt: CtxtRef<'ctxt>,
    ty: Type<'ctxt>,
) -> Result<Layout, LayoutError> {
    Ok(match ty.kind() {
        &TypeKind::Uninit(ty) => {
            let layout = calculate_layout(ctxt, ty)?;

            Layout {
                size: layout.size,
                alignment: layout.alignment,
                kind: LayoutKind::Uninit(Box::new(layout)),
            }
        }
        TypeKind::Infer(_) | TypeKind::Unknown | TypeKind::IntVar(_) => {
            return Err(LayoutError::Unknown);
        }
        TypeKind::Int(integer_kind) => {
            let (signed, size) = integer_kind.signed_and_size();
            Layout::from_scalar(Scalar::integer(signed, size))
        }
        TypeKind::Bool => Layout {
            size: Size::BYTE,
            alignment: Align::BYTE,
            kind: LayoutKind::Scalar(Scalar::Bool),
        },
        TypeKind::Char => Layout::from_scalar(Scalar::integer(false, IntegerSize::Int32)),
        TypeKind::Never => Layout::zst().uninhabited(),
        TypeKind::Param(_, _) => return Err(LayoutError::TooGeneric),
        TypeKind::Function(_) | TypeKind::Box(_) => Layout::pointer(true),
        TypeKind::Array(_) | TypeKind::String => {
            return aggregate_layout(vec![
                (FieldId::new(0), Layout::pointer(true)),
                (
                    FieldId::new(1),
                    Layout::from_scalar(Scalar::uint(IntegerSize::Int64)),
                ),
            ]);
        }
        TypeKind::Tuple(fields) => {
            return record_layout(ctxt, fields.iter().copied().collect());
        }
        TypeKind::Named(id, .., args) => match ctxt.type_def(*id).kind {
            TypeDefKind::Record(fields) => {
                return record_layout(
                    ctxt,
                    fields
                        .into_iter()
                        .map(|field| field.type_of(args, ctxt))
                        .collect(),
                );
            }
            TypeDefKind::Variant(cases) => {
                let tag_ty = ctxt.type_def(*id).tag_type();
                return variant_layout(
                    ctxt,
                    tag_ty,
                    cases
                        .into_iter()
                        .map(|case| case.field.map(|field| field.type_of(args, ctxt)))
                        .collect(),
                );
            }
        },
    })
}
