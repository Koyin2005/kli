use crate::{
    patterns::{
        ctors::Constructor,
        pat::{self, Pat},
    },
    typed_ast::{Pattern, PatternKind},
};

pub fn pattern_to_pat(pattern: &Pattern) -> Pat {
    match &pattern.kind {
        PatternKind::None => Pat {
            constructor: Constructor::None,
            ty: pattern.ty.clone(),
            fields: Vec::new(),
        },
        PatternKind::Some(inner) => Pat {
            ty: pattern.ty.clone(),
            constructor: Constructor::Some,
            fields: vec![pattern_to_pat(inner)],
        },
        PatternKind::Bool(value) => Pat {
            ty: pattern.ty.clone(),
            constructor: Constructor::Bool(*value),
            fields: Vec::new(),
        },
        PatternKind::Binding(.., ty) => Pat {
            ty: (**ty).clone(),
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
        },
        PatternKind::Deref(inner) => Pat {
            ty: pattern.ty.clone(),
            constructor: Constructor::Deref,
            fields: vec![pattern_to_pat(inner)],
        },
    }
}
