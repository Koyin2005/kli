use std::fmt::{Display, Write};

use crate::src_loc::SrcLoc;

#[derive(Debug, Clone)]
pub struct Token {
    pub loc: SrcLoc,
    pub kind: TokenKind,
}
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum IntegerSize {
    Int64,
    Int32,
    Int8,
}
impl IntegerSize {
    pub const fn size_str(self) -> &'static str {
        match self {
            Self::Int32 => "32",
            Self::Int64 => "64",
            Self::Int8 => "8",
        }
    }
}
#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub enum NumberKind {
    Unsigned(IntegerSize),
    Signed(IntegerSize),
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
    DotDot,
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
    Borrow,
    Ident(String),
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
    CharLiteral(char),
    Bor,
    Band,
}

impl Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let txt = match self {
            Self::CharLiteral(c) => {
                f.write_str("\'")?;
                f.write_char(*c)?;
                return f.write_str("\'");
            }
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
            Self::DotDot => "..",
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
            Self::Fun => "fun",
            Self::Borrow => "borrow",
            Self::In => "in",
            Self::String => "string",
            Self::Imm => "imm",
            Self::Ref => "ref",
            Self::Impl => "impl",
            Self::Bor => "bor",
            Self::Band => "band",
            Self::Number(number, sign) => {
                write!(f, "{number}")?;
                return match sign {
                    None => Ok(()),
                    Some(NumberKind::Signed(size)) => write!(f, "{}", size.size_str()),
                    Some(NumberKind::Unsigned(size)) => write!(f, "{}", size.size_str()),
                };
            }
            Self::Mut => "mut",
            Self::Let => "let",
            Self::Case => "case",
            Self::End => "end",
            Self::Ident(name) => name,
            Self::Error => "{error}",
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
