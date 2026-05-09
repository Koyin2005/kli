use crate::{
    patterns::{ctors::Constructor, pat::Pat},
    typed_ast::{Pattern, PatternKind},
};

pub fn pattern_to_pat(pattern: &Pattern) -> Pat {
    match &pattern.kind {
        PatternKind::None => Pat {
            constructor: Constructor::None,
            fields: Vec::new(),
        },
        PatternKind::Some(inner) => Pat {
            constructor: Constructor::Some,
            fields: vec![pattern_to_pat(inner)],
        },
        PatternKind::Bool(value) => Pat {
            constructor: Constructor::Bool(*value),
            fields: Vec::new(),
        },
        PatternKind::Binding(..) => Pat {
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
        },
        PatternKind::Deref(inner) => Pat {
            constructor: Constructor::Deref,
            fields: vec![pattern_to_pat(inner)],
        },
    }
}
