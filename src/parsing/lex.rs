use std::{iter::Peekable, num::IntErrorKind, str::CharIndices};

use crate::{
    diagnostics::DiagnosticReporter,
    ident::Symbol,
    parsing::tokens::{NumberKind, Token, TokenKind},
    src_loc::SrcLoc,
};

pub struct Lexer<'src> {
    chars: Peekable<CharIndices<'src>>,
    src: &'src str,
    file: Symbol,
    line: u32,
    index: u32,
    start_line: u32,
    start_index: u32,
    diag: DiagnosticReporter,
}
impl<'s> Lexer<'s> {
    pub fn new(file: Symbol, src: &'s str) -> Self {
        Self {
            file,
            index: 0,
            start_index: 0,
            src,
            chars: src.char_indices().peekable(),
            line: 1,
            start_line: 1,
            diag: DiagnosticReporter::new(),
        }
    }
    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().copied().map(|(_, c)| c)
    }
    fn next_char(&mut self) -> Option<char> {
        self.chars.next().map(|(index, c)| {
            self.index = (index + c.len_utf8()) as u32;
            if c == '\n' {
                self.line = self.line.checked_add(1).expect("file too big");
            }
            c
        })
    }
    fn match_char(&mut self, c: char) -> Option<char> {
        if self.peek_char().is_some_and(|p| p == c) {
            self.next_char()
        } else {
            None
        }
    }
    fn match_char_with(&mut self, f: impl FnOnce(char) -> bool) -> Option<char> {
        if self.peek_char().is_some_and(f) {
            self.next_char()
        } else {
            None
        }
    }
    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek_char() {
            if c.is_whitespace() {
                self.next_char();
            } else if c == '#' {
                self.next_char();
                while self.match_char_with(|c| c != '\n').is_some() {}
            } else {
                break;
            }
        }
    }
    fn current_loc(&self) -> SrcLoc {
        SrcLoc {
            line: self.start_line,
            file: self.file,
        }
    }
    fn next_token_from_char(&mut self, kind: TokenKind) -> Token {
        self.next_char();
        self.new_token(kind)
    }
    fn next_token_from_char_or_char_match<const N: usize>(
        &mut self,
        single_kind: TokenKind,
        next_kinds: [(char, TokenKind); N],
    ) -> Token {
        self.next_char();
        let kind = next_kinds
            .into_iter()
            .find_map(|(c, kind)| self.match_char(c).map(|_| kind));
        self.new_token(kind.unwrap_or(single_kind))
    }
    fn next_token_from_char_or_chars(
        &mut self,
        next_char: char,
        single_kind: TokenKind,
        double_kind: TokenKind,
    ) -> Token {
        self.next_char();
        let kind = if self.match_char(next_char).is_some() {
            double_kind
        } else {
            single_kind
        };
        self.new_token(kind)
    }
    fn new_token(&self, kind: TokenKind) -> Token {
        Token {
            loc: self.current_loc(),
            kind,
        }
    }
    fn is_start_char(c: char) -> bool {
        c.is_ascii_alphabetic() || c == '_'
    }
    fn is_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }
    fn num_token(&mut self) -> Option<Token> {
        let mut src = String::new();
        while let Some(c) = self.match_char_with(|c| char::is_digit(c, 10)) {
            src.push(c);
        }
        let sign = match self.peek_char() {
            Some('u') => Some(NumberKind::Unsigned),
            Some('i') => Some(NumberKind::Signed),
            _ => None,
        };
        if sign.is_some() {
            self.next_char();
        }
        match src.parse::<u64>() {
            Ok(n) => Some(self.new_token(TokenKind::Number(n, sign))),
            Err(e) => match e.kind() {
                IntErrorKind::PosOverflow => {
                    let loc = self.current_loc();
                    self.diag.add_diagnostic("Integer too large", loc);
                    Some(Token {
                        loc,
                        kind: TokenKind::Number(u64::MAX, sign),
                    })
                }
                _ => None,
            },
        }
    }
    fn string_token(&mut self) -> Option<Token> {
        self.next_char();
        let mut src = String::new();
        let mut prev_char = None;
        while let Some(c) = self.peek_char()
            && c != '"'
        {
            if let Some('\\') = prev_char {
                src.pop();
                src.push(match c {
                    '\\' => '\\',
                    'n' => '\n',
                    't' => '\t',
                    _ => {
                        self.diag.add_diagnostic(
                            format!("invalid escape character '{}'", c),
                            self.current_loc(),
                        );
                        self.next_char();
                        prev_char = Some(c);
                        continue;
                    }
                });
            } else {
                src.push(c);
            }
            self.next_char();

            prev_char = Some(c);
        }
        if self.match_char('"').is_some() {
            Some(Token {
                loc: self.current_loc(),
                kind: TokenKind::StringLiteral(src),
            })
        } else {
            let loc = self.current_loc();
            self.diag
                .add_diagnostic("Expected '\"' at end of string", loc);
            None
        }
    }
    fn char_literal(&mut self) -> Option<Token> {
        self.next_char()?;
        '"';
        let c = if self.match_char('\\').is_some() {
            match self.peek_char()? {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                '\'' => '\'',
                c => {
                    self.diag.add_diagnostic(
                        format!("Invalid character escape '{c}'",),
                        self.current_loc(),
                    );
                    return None;
                }
            }
        } else {
            self.next_char()?
        };
        if self.match_char('\'').is_some() {
            Some(Token {
                loc: self.current_loc(),
                kind: TokenKind::CharLiteral(c),
            })
        } else {
            self.diag
                .add_diagnostic(format!("Expected ' at end of char",), self.current_loc());
            return None;
        }
    }
    fn current_token_src(&self) -> &str {
        &self.src[self.start_index as usize..self.index as usize]
    }
    fn ident_token(&mut self) -> Option<Token> {
        self.next_char()?;
        while self.match_char_with(Self::is_ident_char).is_some() {}
        Some(Token {
            loc: self.current_loc(),
            kind: match self.current_token_src() {
                "addr_of" => TokenKind::AddrOf,
                "fun" => TokenKind::Fun,
                "imm" => TokenKind::Imm,
                "mut" => TokenKind::Mut,
                "borrow" => TokenKind::Borrow,
                "in" => TokenKind::In,
                "for" => TokenKind::For,
                "panic" => TokenKind::Panic,
                "int" => TokenKind::Int,
                "uint" => TokenKind::Uint,
                "unsafe" => TokenKind::Unsafe,
                "string" => TokenKind::String,
                "bool" => TokenKind::Bool,
                "let" => TokenKind::Let,
                "case" => TokenKind::Case,
                "static" => TokenKind::Static,
                "ref" => TokenKind::Ref,
                "impl" => TokenKind::Impl,
                "char" => TokenKind::Char,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "end" => TokenKind::End,
                "of" => TokenKind::Of,
                "do" => TokenKind::Do,
                "type" => TokenKind::Type,
                "while" => TokenKind::While,
                "with" => TokenKind::With,
                "import" => TokenKind::Import,
                "as" => TokenKind::As,
                "and" => TokenKind::And,
                "return" => TokenKind::Return,
                "or" => TokenKind::Or,
                _ => TokenKind::Ident(self.current_token_src().to_string()),
            },
        })
    }
    fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();
        let line = self.line;
        let c = self.peek_char()?;
        self.start_line = line;
        self.start_index = self.index;
        match c {
            '@' => Some(self.next_token_from_char(TokenKind::At)),
            '.' => Some(self.next_token_from_char_or_chars('.', TokenKind::Dot, TokenKind::DotDot)),
            '=' => Some(self.next_token_from_char_or_char_match(
                TokenKind::Equal,
                [('>', TokenKind::ThickArrow), ('=', TokenKind::DoubleEqual)],
            )),
            '(' => Some(self.next_token_from_char(TokenKind::LeftParen)),
            ')' => Some(self.next_token_from_char(TokenKind::RightParen)),
            '{' => Some(self.next_token_from_char(TokenKind::LeftBrace)),
            '}' => Some(self.next_token_from_char(TokenKind::RightBrace)),
            '[' => Some(self.next_token_from_char(TokenKind::LeftBracket)),
            ']' => Some(self.next_token_from_char(TokenKind::RightBracket)),
            '+' => Some(self.next_token_from_char(TokenKind::Plus)),
            '-' => {
                Some(self.next_token_from_char_or_chars('>', TokenKind::Minus, TokenKind::Arrow))
            }
            '\'' => self.char_literal(),
            '/' => Some(self.next_token_from_char(TokenKind::Slash)),
            '*' => Some(self.next_token_from_char(TokenKind::Star)),
            ',' => Some(self.next_token_from_char(TokenKind::Coma)),
            ';' => Some(self.next_token_from_char(TokenKind::Semi)),
            ':' => Some(self.next_token_from_char(TokenKind::Colon)),
            '^' => Some(self.next_token_from_char(TokenKind::Caret)),
            '|' => Some(self.next_token_from_char(TokenKind::Pipe)),
            c if Self::is_start_char(c) => self.ident_token(),
            c if c.is_numeric() => self.num_token(),
            '"' => self.string_token(),
            '<' => Some(self.next_token_from_char(TokenKind::Lesser)),
            '>' => Some(self.next_token_from_char(TokenKind::Greater)),
            _ => {
                self.diag
                    .add_diagnostic(format!("Unrecognised char '{}'", c), self.current_loc());
                Some(self.next_token_from_char(TokenKind::Error))
            }
        }
    }
    pub fn lex(mut self) -> (bool, Vec<Token>, Token) {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token() {
            tokens.push(token);
        }
        let eof_token = Token {
            loc: self.current_loc(),
            kind: TokenKind::Eof,
        };
        let error = self.diag.report_all();
        (error, tokens, eof_token)
    }
}
