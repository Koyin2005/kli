use crate::{
    collect::CtxtRef,
    patterns::{ctors::Constructor, pat::Pat},
    typed_ast::{Pattern, PatternKind},
};

pub fn pattern_to_pat(ctxt: CtxtRef<'_>, pattern: &Pattern) -> Pat {
    let ty = pattern.ty.clone();

    match &pattern.kind {
        PatternKind::Int(value) => Pat {
            ty,
            constructor: Constructor::Int(*value as i128),
            fields: Vec::new(),
        },
        PatternKind::Unit => Pat {
            ty,
            constructor: Constructor::Unit,
            fields: Vec::new(),
        },
        PatternKind::Ref(inner) => Pat {
            ty,
            constructor: Constructor::Ref,
            fields: vec![pattern_to_pat(ctxt, inner).with_index(0)],
        },
        PatternKind::Record(fields) => Pat {
            ty,
            constructor: Constructor::Record,
            fields: fields
                .iter()
                .map(|field| {
                    pattern_to_pat(ctxt, &field.pattern).with_index(field.index.into_usize())
                })
                .collect(),
        },
        PatternKind::Bool(value) => Pat {
            constructor: Constructor::Bool(*value),
            fields: Vec::new(),
            ty,
        },
        PatternKind::Case(id, _, _, inner) => Pat {
            ty,
            constructor: Constructor::Case(ctxt.expect_ident(*id).symbol),
            fields: inner
                .as_ref()
                .map(|inner| pattern_to_pat(ctxt, inner).with_index(0))
                .into_iter()
                .collect(),
        },
        PatternKind::Binding(..) | PatternKind::Err => Pat {
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
            ty,
        },
    }
}
