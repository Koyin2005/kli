use std::fmt::Display;

use crate::src_loc::SrcLoc;

#[derive(Debug, Clone)]
pub struct Token {
    pub loc: SrcLoc,
    pub kind: TokenKind,
}
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum NumberKind {
    Unsigned,
    Signed,
}
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TokenKind {
    Or,
    With,
    LeftBrace,
    RightBrace,
    Equal,
    Coma,
    Plus,
    Minus,
    Slash,
    End,
    Star,
    Caret,
    Of,
    Do,
    Dot,
    Lesser,
    Greater,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Pipe,
    Number(u64, Option<NumberKind>),
    Impl,
    Semi,
    Colon,
    Fun,
    Char,
    Print,
    Borrow,
    Ident(String),
    Int,
    Uint,
    Bool,
    String,
    StringLiteral(String),
    Ref,
    Static,
    Let,
    Mut,
    Imm,
    Case,
    In,
    Panic,
    For,
    Arrow,
    ThickArrow,
    DoubleEqual,
    True,
    False,
    Error,
    Region,
    While,
    At,
    Type,
    As,
    AddrOf,
    Import,
    Eof,
    And,
    Return,
    Unsafe,
}

impl Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let txt = match self {
            Self::At => "@",
            Self::Semi => ";",
            Self::LeftBrace => "{",
            Self::RightBrace => "}",
            Self::Coma => ",",
            Self::Arrow => "->",
            Self::ThickArrow => "=>",
            Self::Bool => "bool",
            Self::False => "false",
            Self::Minus => "-",
            Self::True => "true",
            Self::Star => "*",
            Self::Plus => "+",
            Self::Equal => "=",
            Self::Pipe => "|",
            Self::Of => "of",
            Self::Do => "do",
            Self::Dot => ".",
            Self::AddrOf => "addr_of",
            Self::DoubleEqual => "==",
            Self::Lesser => "<",
            Self::Greater => ">",
            Self::StringLiteral(literal) => {
                f.write_str("\"")?;
                f.write_str(literal)?;
                return f.write_str("\"");
            }
            Self::While => "while",
            Self::LeftParen => "(",
            Self::RightParen => ")",
            Self::LeftBracket => "[",
            Self::RightBracket => "]",
            Self::Caret => "^",
            Self::Panic => "panic",
            Self::For => "for",
            Self::Static => "static",
            Self::Colon => ":",
            Self::Slash => "/",
            Self::Char => "char",
            Self::Print => "print",
            Self::Fun => "fun",
            Self::Borrow => "borrow",
            Self::In => "in",
            Self::Int => "int",
            Self::Uint => "uint",
            Self::String => "string",
            Self::Imm => "imm",
            Self::Ref => "ref",
            Self::Impl => "impl",
            Self::Number(number, sign) => {
                return write!(
                    f,
                    "{number}{}",
                    match sign {
                        Some(NumberKind::Signed) => "i",
                        Some(NumberKind::Unsigned) => "u",
                        None => "",
                    }
                );
            }
            Self::Mut => "mut",
            Self::Let => "let",
            Self::Case => "case",
            Self::End => "end",
            Self::Ident(name) => name,
            Self::Error => "{error}",
            Self::Region => "region",
            Self::Import => "import",
            Self::Type => "type",
            Self::Eof => "EOF",
            Self::With => "with",
            Self::As => "as",
            Self::And => "and",
            Self::Return => "return",
            Self::Unsafe => "unsafe",
            Self::Or => "or",
        };
        f.write_str(txt)
    }
}
