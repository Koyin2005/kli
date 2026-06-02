use crate::{
    patterns::{ctors::Constructor, pat::Pat},
    typed_ast::{Pattern, PatternKind},
};

pub fn pattern_to_pat<'a>(pattern: &'a Pattern) -> Pat<'a> {
    let ty = &pattern.ty;

    match &pattern.kind {
        PatternKind::Record(fields) => Pat {
            ty,
            constructor: Constructor::Record,
            fields: fields
                .iter()
                .map(|field| pattern_to_pat(&field.pattern).with_index(field.index.into_index()))
                .collect(),
        },
        PatternKind::None => Pat {
            constructor: Constructor::None,
            fields: Vec::new(),
            ty,
        },
        PatternKind::Some(inner) => Pat {
            constructor: Constructor::Some,
            fields: vec![pattern_to_pat(inner).with_index(0)],
            ty,
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
