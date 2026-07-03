use crate::{
    collect::CtxtRef,
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
    pub fn wildcard(ty: &Type) -> Self {
        Self {
            ty: ty.clone(),
            constructor: Constructor::Wildcard,
            fields: Vec::new(),
        }
    }
    pub fn with_index(self, index: usize) -> PatWithIndex {
        PatWithIndex { pat: self, index }
    }
    pub fn format(&self, ctxt: CtxtRef<'_>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.constructor {
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
                let fields: &mut dyn Fn(FieldId) -> crate::types::FieldName = match &self.ty {
                    Type::Record(fields) => &mut |i| fields[i].name,
                    &Type::Named(id, ..) => &mut move |i| {
                        crate::types::FieldName::Named(ctxt.type_def(id).fields()[i].name)
                    },
                    _ => unreachable!("should be a record"),
                };
                f.write_str("{")?;
                let mut first = true;

                for pat in self.fields.iter() {
                    if !first {
                        f.write_str(", ")?;
                    }

                    write!(f, "{} = ", fields(FieldId::new(pat.index)))?;
                    pat.pat.format(ctxt, f)?;
                    first = false;
                }
                f.write_str("}")
            }
        }
    }
}

pub fn missing_patterns(
    ctxt: CtxtRef<'_>,
    ty: &[Type; 1],
    patterns: &mut dyn Iterator<Item = Pat>,
) -> Vec<Pat> {
    let missing = missing_patterns_inner(ctxt, ty, patterns.map(|pat| vec![pat]).collect());
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
                let mut new_row = fields.iter().map(Pat::wildcard).collect::<Vec<_>>();
                for indexed_pat in first.fields {
                    new_row[indexed_pat.index] = indexed_pat.pat;
                }
                new_row.extend(row);
                Some(new_row)
            } else if first.constructor == Constructor::Wildcard {
                let mut new_row = fields.iter().map(Pat::wildcard).collect::<Vec<_>>();
                new_row.extend(row);
                Some(new_row)
            } else {
                None
            }
        })
        .collect()
}
fn missing_patterns_inner(
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
    let constructors = constructors_of_ty(ctxt, head);
    let mut all_missing = Vec::new();
    for c in constructors {
        let fields = fields_of(head, c, ctxt);
        let field_count = fields.len();
        let specialized = specialize(c, &fields, matrix.clone());
        let missing = missing_patterns_inner(
            ctxt,
            &fields.iter().chain(&tys[1..]).cloned().collect::<Vec<_>>(),
            specialized,
        );
        for row in missing {
            let mut row = row.into_iter();
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
    all_missing
}
