use crate::{
    patterns::{ctors::Constructor, pat::Pat},
    typed_ast::{Pattern, PatternKind},
};

pub fn pattern_to_pat<'a>(pattern: &'a Pattern) -> Pat<'a> {
    let ty = &pattern.ty;

    match &pattern.kind {
        PatternKind::Int(value) => Pat {
            ty,
            constructor: Constructor::Int(*value),
            fields: Vec::new(),
        },
        PatternKind::Ref(inner) => Pat {
            ty,
            constructor: Constructor::Ref,
            fields: vec![pattern_to_pat(inner).with_index(0)],
        },
        PatternKind::Record(fields) => Pat {
            ty,
            constructor: Constructor::Record,
            fields: fields
                .iter()
                .map(|field| pattern_to_pat(&field.pattern).with_index(field.index.into_usize()))
                .collect(),
        },
        PatternKind::Bool(value) => Pat {
            constructor: Constructor::Bool(*value),
            fields: Vec::new(),
            ty,
        },
        PatternKind::Binding(..) => Pat {
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
            ty,
        },
    }
}
