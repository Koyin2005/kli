pub struct Token{
    kind : TokenKind
}
pub enum TokenKind {
    Ident(String),
    Fun,
    Arrow,
    LeftBrace,
    RightBrace,
    Let,
    Mut,
    Equal,
    Imm,
    Some,
    In,
    None,
    Panic,
    Coma,
    Print,
    Borrow,
    For,
    Plus,
    Minus,
    Slash,
    Star,
}