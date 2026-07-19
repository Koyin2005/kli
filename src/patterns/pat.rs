use std::collections::HashSet;

use crate::{
    collect::CtxtRef,
    def_ids::DefId,
    patterns::ctors::{Constructor, constructors_of_ty, fields_of},
    types::Type,
};
#[derive(Clone)]
pub struct PatWithIndex {
    pub pat: Pat,
    pub index: usize,
}
#[derive(Clone)]
pub struct Pat {
    pub ty: Type,
    pub constructor: Constructor,
    pub fields: Vec<PatWithIndex>,
}
impl Pat {
    pub fn wildcard(ty: Type) -> Self {
        Self {
            ty,
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
        }
    }
    pub fn with_index(self, index: usize) -> PatWithIndex {
        PatWithIndex { pat: self, index }
    }
    pub fn format(&self, ctxt: CtxtRef<'_>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.constructor {
            Constructor::Missing => f.write_str("missing"),
            Constructor::Bool(value) => {
                if value {
                    f.write_str("true")
                } else {
                    f.write_str("false")
                }
            }
            Constructor::Int(value) => {
                write!(f, "{}", value)
            }
            Constructor::Ref => {
                f.write_str("ref ")?;
                self.fields[0].pat.format(ctxt, f)
            }
            Constructor::Case(name) => {
                if let Some(field) = self.fields.first() {
                    write!(f, "{}(", name)?;
                    field.pat.format(ctxt, f)?;
                    write!(f, ")")
                } else {
                    write!(f, "{}", name)
                }
            }
            Constructor::Wildcard => f.write_str("_"),
            Constructor::NonExhaustive => f.write_str("_"),
            Constructor::Record => {
                use crate::typed_ast::FieldId;
                let (fields, brackets): (&mut dyn Fn(FieldId) -> _, _) = match &self.ty {
                    Type::Record(fields) => (&mut |i| Some(fields[i].name), ("{", "}")),
                    &Type::Named(id, ..) => (
                        &mut move |i| {
                            Some(crate::types::FieldName::Named(
                                ctxt.type_def(id).fields()[i].name,
                            ))
                        },
                        ("{", "}"),
                    ),
                    Type::Tuple(fields) => (
                        &mut |_| None,
                        ("(", if fields.len() == 1 { ",)" } else { ")" }),
                    ),
                    _ => unreachable!("should be a record"),
                };
                let (start, end) = brackets;
                f.write_str(start)?;
                let mut first = true;

                for pat in self.fields.iter() {
                    if !first {
                        f.write_str(", ")?;
                    }
                    if let Some(name) = fields(FieldId::new(pat.index)) {
                        write!(f, "{} = ", name)?;
                    }
                    pat.pat.format(ctxt, f)?;
                    first = false;
                }
                f.write_str(end)
            }
        }
    }
}

pub fn missing_patterns(
    from_id: DefId,
    ctxt: CtxtRef<'_>,
    ty: &[Type; 1],
    patterns: &mut dyn Iterator<Item = Pat>,
) -> Vec<Pat> {
    let missing =
        missing_patterns_inner(from_id, ctxt, ty, patterns.map(|pat| vec![pat]).collect());
    missing
        .into_iter()
        .map(|mut row| row.swap_remove(0))
        .collect()
}

fn specialize(constructor: Constructor, fields: &[Type], matrix: Vec<Vec<Pat>>) -> Vec<Vec<Pat>> {
    matrix
        .into_iter()
        .filter_map(|mut row| {
            let first = (if row.is_empty() {
                None
            } else {
                Some(row.remove(0))
            })?;
            if first.constructor == constructor {
                let mut new_row = fields
                    .iter()
                    .cloned()
                    .map(Pat::wildcard)
                    .collect::<Vec<_>>();
                for indexed_pat in first.fields {
                    new_row[indexed_pat.index] = indexed_pat.pat;
                }
                new_row.extend(row);
                Some(new_row)
            } else if first.constructor == Constructor::Wildcard {
                let mut new_row = fields
                    .iter()
                    .cloned()
                    .map(Pat::wildcard)
                    .collect::<Vec<_>>();
                new_row.extend(row);
                Some(new_row)
            } else {
                None
            }
        })
        .collect()
}
fn split_constructors(
    ty: &Type,
    all_constructors: Vec<Constructor>,
    seen_constructors: HashSet<Constructor>,
) -> (Vec<Constructor>, Vec<Constructor>) {
    let mut seen = Vec::new();
    let mut missing = Vec::new();
    let had_non_exhaustive = all_constructors.contains(&Constructor::NonExhaustive);
    if had_non_exhaustive {
        missing.push(Constructor::NonExhaustive);
    }
    match ty {
        Type::Infer(_) | Type::Unknown => (),
        Type::Never => (),
        Type::Int(_)
        | Type::Char
        | Type::Byte
        | Type::Param(..)
        | Type::Function(..)
        | Type::Array(..)
        | Type::RawPointer(_) => {}
        Type::Bool => {
            let is_true = seen_constructors.contains(&Constructor::Bool(true));
            let is_false = seen_constructors.contains(&Constructor::Bool(false));
            if is_true {
                seen.push(Constructor::Bool(true));
            } else {
                missing.push(Constructor::Bool(true));
            }
            if is_false {
                seen.push(Constructor::Bool(false));
            } else {
                missing.push(Constructor::Bool(false));
            }
        }
        Type::Imm(..) | Type::Mut(..) => {
            if seen_constructors.contains(&Constructor::Ref) {
                seen.push(Constructor::Ref);
            } else {
                missing.push(Constructor::Ref);
            }
        }
        Type::Record(_) | Type::Tuple(_) => {
            if seen_constructors.contains(&Constructor::Record) {
                seen.push(Constructor::Record);
            } else {
                missing.push(Constructor::Record);
            }
        }
        Type::Named(..) => {
            if !had_non_exhaustive {
                for ctor in all_constructors {
                    match ctor {
                        Constructor::Record => {
                            if seen_constructors.contains(&Constructor::Record) {
                                seen.push(Constructor::Record)
                            } else {
                                missing.push(Constructor::Record);
                            }
                        }
                        Constructor::Case(_) => {
                            if seen_constructors.contains(&ctor) {
                                seen.push(ctor)
                            } else {
                                missing.push(ctor);
                            }
                        }
                        _ => continue,
                    }
                }
            }
        }
    }
    (seen, missing)
}
fn missing_patterns_inner(
    from_id: DefId,
    ctxt: CtxtRef<'_>,
    tys: &'_ [Type],
    matrix: Vec<Vec<Pat>>,
) -> Vec<Vec<Pat>> {
    let Some(head) = tys.first() else {
        return if matrix.is_empty() {
            vec![Vec::new()]
        } else {
            Vec::new()
        };
    };
    let all_constructors = constructors_of_ty(from_id, ctxt, head);
    let (mut constructors, missing_ctors) = split_constructors(
        head,
        all_constructors,
        matrix
            .iter()
            .filter_map(|row| row.first().map(|first| first.constructor))
            .collect(),
    );
    if !missing_ctors.is_empty() {
        constructors.push(Constructor::Missing);
    }
    let mut all_missing = Vec::new();
    for c in constructors {
        let fields = fields_of(head, c, ctxt);
        let field_count = fields.len();
        let specialized = specialize(c, &fields, matrix.clone());
        let missing = missing_patterns_inner(
            from_id,
            ctxt,
            &fields.iter().chain(&tys[1..]).cloned().collect::<Vec<_>>(),
            specialized,
        );

        for row in missing {
            let mut row = row.into_iter();
            if c == Constructor::Missing {
                all_missing.extend(missing_ctors.iter().copied().map(|ctor| {
                    std::iter::once(Pat {
                        ty: head.clone(),
                        constructor: ctor,
                        fields: fields_of(head, ctor, ctxt)
                            .into_iter()
                            .map(Pat::wildcard)
                            .enumerate()
                            .map(|(i, pat)| pat.with_index(i))
                            .collect(),
                    })
                    .chain(row.clone())
                    .collect()
                }));
            } else {
                let head_pat = Pat {
                    ty: head.clone(),
                    constructor: c,
                    fields: row
                        .by_ref()
                        .take(field_count)
                        .enumerate()
                        .map(|(i, pat)| pat.with_index(i))
                        .collect(),
                };
                let mut new_row = vec![head_pat];
                new_row.extend(row);
                all_missing.push(new_row);
            }
        }
    }
    all_missing
}
