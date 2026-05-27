use std::fmt::{Debug, Display};

use crate::{
    patterns::ctors::{Constructor, constructors_of_ty, fields_of},
    types::Type,
};
#[derive(Clone)]
pub struct PatWithIndex<'a> {
    pub pat: Pat<'a>,
    pub index: usize,
}
#[derive(Clone)]
pub struct Pat<'a> {
    pub ty: &'a Type,
    pub constructor: Constructor,
    pub fields: Vec<PatWithIndex<'a>>,
}
impl<'a> Pat<'a> {
    pub const fn wildcard(ty: &'a Type) -> Self {
        Self {
            ty,
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
        }
    }
    pub fn with_index(self, index: usize) -> PatWithIndex<'a> {
        PatWithIndex { pat: self, index }
    }
}
impl Debug for Pat<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}
impl Display for Pat<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.constructor {
            Constructor::Bool(value) => {
                if value {
                    f.write_str("true")
                } else {
                    f.write_str("false")
                }
            }
            Constructor::Deref => {
                let pat = &self.fields[0];
                f.write_str("^")?;
                write!(f, "{}", pat.pat)
            }
            Constructor::None => f.write_str("None"),
            Constructor::Some => {
                f.write_str("Some(")?;
                write!(f, "{}", self.fields[0].pat)?;
                f.write_str(")")
            }
            Constructor::Wildcard => f.write_str("_"),
            Constructor::NonExhaustive => f.write_str("_"),
            Constructor::Record => {
                let Type::Record(fields) = self.ty else {
                    unreachable!("Should be a record")
                };
                f.write_str("{")?;
                let mut first = true;

                for (field, pat) in fields.iter().zip(&self.fields) {
                    if !first {
                        f.write_str(", ")?;
                    }
                    write!(f, "{} = {}", field.name, pat.pat)?;
                    first = false;
                }
                f.write_str("}")
            }
        }
    }
}

pub fn missing_patterns<'a>(
    ty: &'a [&'a Type; 1],
    patterns: &mut dyn Iterator<Item = Pat<'a>>,
) -> Vec<Pat<'a>> {
    let missing = missing_patterns_inner(ty, patterns.map(|pat| vec![pat]).collect());
    missing
        .into_iter()
        .map(|mut row| row.swap_remove(0))
        .collect()
}

fn specialize<'a>(
    constructor: Constructor,
    fields: &'a [&'a Type],
    matrix: Vec<Vec<Pat<'a>>>,
) -> Vec<Vec<Pat<'a>>> {
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
                    .copied()
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
                    .copied()
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
fn missing_patterns_inner<'b>(tys: &[&'b Type], matrix: Vec<Vec<Pat>>) -> Vec<Vec<Pat<'b>>> {
    let Some(&head) = tys.first() else {
        return if matrix.is_empty() {
            vec![Vec::new()]
        } else {
            Vec::new()
        };
    };
    let constructors = constructors_of_ty(head);
    let mut all_missing = Vec::new();
    for c in constructors {
        let fields = fields_of(head, c);
        let field_count = fields.len();
        let specialized = specialize(c, &fields, matrix.clone());
        let missing = missing_patterns_inner(
            &fields.iter().chain(&tys[1..]).copied().collect::<Vec<_>>(),
            specialized,
        );
        for row in missing {
            let mut row = row.into_iter();
            let head_pat = Pat {
                ty: head,
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
    all_missing
}
