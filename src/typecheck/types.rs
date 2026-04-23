use std::fmt::Display;

#[derive(PartialEq, Eq)]
pub enum Type {
    Unknown,
    Unit,
    Int,
    Bool,
    String,
    Ref(Box<Type>),
    Function(Vec<Type>,Box<Type>)
}

impl Display for Type{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self{
            Self::Bool => f.pad("bool"),
            Self::Int => f.pad("int"),
            Self::Unit => f.pad("unit"),
            Self::Unknown => f.pad("{unknown}"),
            Self::String => f.pad("string"),
            Self::Ref(ty) => {
                f.pad("ref[")?;
                ty.fmt(f)?;
                f.pad("]")
            },
            Self::Function(params,return_type) => {
                f.pad("fun(")?;
                let mut first = true;
                for param in params{
                    if !first{
                        f.pad(",")?;
                    }
                    param.fmt(f)?;
                    first = false;
                }
                f.pad(") -> ")?;
                return_type.fmt(f)?;
                f.pad("]")
            },
        }
    }
}