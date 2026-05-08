use std::fmt::Display;

use crate::{
    patterns::ctors::{Constructor, constructors_of_ty, fields_of},
    types::Type,
};
#[derive(Clone)]
pub struct Pat {
    pub ty: Type,
    pub constructor: Constructor,
    pub fields: Vec<Pat>,
}
impl Display for Pat{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.constructor{
            Constructor::Bool(value) => if value{
                f.write_str("true")
            } else {
                f.write_str("false")
            },
            Constructor::Deref => {
                let pat = &self.fields[0];
                f.write_str("^")?;
                write!(f,"{}",pat)
            },
            Constructor::None => f.write_str("None"),
            Constructor::Some => {
                f.write_str("Some(")?;
                write!(f,"{}",self.fields[0])?;
                f.write_str(")")
            },
            Constructor::Wildcard => f.write_str("_"),
            Constructor::NonExhaustive => f.write_str("_")
        }
    }
}

pub fn missing_patterns(ty: &Type, patterns: &mut dyn Iterator<Item = Pat>) -> Vec<Pat> {
    let missing = missing_patterns_inner(
        core::slice::from_ref(ty),
        patterns.map(|pat| vec![pat]).collect(),
    );
    missing
        .into_iter()
        .map(|mut row| row.swap_remove(0))
        .collect()
}

fn specialize(constructor: Constructor, fields: &[Type], matrix: Vec<Vec<Pat>>) -> Vec<Vec<Pat>> {
    matrix
        .into_iter()
        .filter_map(|mut row| {
            let Some(first) = (if row.is_empty() {
                None
            } else {
                Some(row.remove(0))
            }) else {
                return None;
            };
            if first.constructor == constructor {
                let mut new_row = first.fields;
                new_row.reserve(row.len());
                new_row.extend(row);
                Some(new_row)
            } else if first.constructor == Constructor::Wildcard {
                let mut new_row = fields
                    .iter()
                    .map(|ty| Pat {
                        ty: ty.clone(),
                        constructor: Constructor::Wildcard,
                        fields: Vec::new(),
                    })
                    .collect::<Vec<_>>();
                new_row.extend(row);
                Some(new_row)
            } else {
                None
            }
        })
        .collect()
}
fn missing_patterns_inner(tys: &[Type], matrix: Vec<Vec<Pat>>) -> Vec<Vec<Pat>> {
    let Some(head) = tys.get(0) else {
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
        let specialized = specialize(c, fields, matrix.clone());
        let missing = missing_patterns_inner(fields, specialized);
        for row in missing {
            let mut row = row.into_iter();
            let head_pat = Pat {
                ty: head.clone(),
                constructor: c,
                fields: row.by_ref().take(fields.len()).collect(),
            };
            let mut new_row = vec![head_pat];
            new_row.extend(row);
            all_missing.push(new_row);
        }
    }
    all_missing
}
