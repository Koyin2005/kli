use crate::{
    patterns::{ctors::Constructor, pat::Pat},
    typed_ast::{Pattern, PatternKind},
};

pub fn pattern_to_pat<'a>(pattern: &'a Pattern) -> Pat<'a> {
    let ty = &pattern.ty;
    match &pattern.kind {
        PatternKind::None => Pat {
            constructor: Constructor::None,
            fields: Vec::new(),
            ty,
        },
        PatternKind::Some(inner) => Pat {
            constructor: Constructor::Some,
            fields: vec![pattern_to_pat(inner)],
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
        PatternKind::Deref(inner) => Pat {
            constructor: Constructor::Deref,
            fields: vec![pattern_to_pat(inner)],
            ty,
        },
    }
}
