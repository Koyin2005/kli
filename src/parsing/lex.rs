use std::{iter::Peekable, num::IntErrorKind, rc::Rc, str::Chars};

use crate::{
    diagnostics::DiagnosticReporter,
    parsing::tokens::{Token, TokenKind},
    src_loc::SrcLoc,
};

pub struct Lexer<'src> {
    chars: Peekable<Chars<'src>>,
    file: Rc<str>,
    line: usize,
    start_line: usize,
    diag: DiagnosticReporter,
}
impl<'s> Lexer<'s> {
    pub fn new(file: Rc<str>, src: &'s str) -> Self {
        Self {
            file,
            chars: src.chars().peekable(),
            line: 1,
            start_line: 1,
            diag: DiagnosticReporter::new(),
        }
    }
    fn peek_char(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }
    fn next_char(&mut self) -> Option<char> {
        self.chars.next().inspect(|x| {
            if *x == '\n' {
                self.line += 1;
            }
        })
    }
    fn match_char(&mut self, c: char) -> Option<char> {
        if self.chars.peek().is_some_and(|p| *p == c) {
            self.next_char()
        } else {
            None
        }
    }
    fn match_char_with(&mut self, f: impl FnOnce(char) -> bool) -> Option<char> {
        if let Some(&c) = self.chars.peek()
            && f(c)
        {
            self.next_char()
        } else {
            None
        }
    }
    fn skip_whitespace(&mut self) {
        while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
            self.next_char();
        }
    }
    fn current_loc(&self) -> SrcLoc {
        SrcLoc {
            line: self.start_line,
            file: self.file.clone(),
        }
    }
    fn next_token_from_char(&mut self, kind: TokenKind) -> Token {
        self.next_char();
        self.new_token(kind)
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
        match src.parse::<u64>() {
            Ok(n) => Some(self.new_token(TokenKind::Number(n))),
            Err(e) => match e.kind() {
                IntErrorKind::PosOverflow => {
                    let loc = self.current_loc();
                    self.diag
                        .add_diagnostic("Integer too large".to_string(), loc.clone());
                    Some(Token {
                        loc,
                        kind: TokenKind::Number(u64::MAX),
                    })
                }
                _ => None,
            },
        }
    }
    fn string_token(&mut self) -> Option<Token> {
        self.next_char();
        let mut src = String::new();
        while let Some(c) = self.peek_char()
            && c != '"'
        {
            src.push(c);
            self.next_char();
        }

        if self.match_char('"').is_some() {
            Some(Token {
                loc: self.current_loc(),
                kind: TokenKind::StringLiteral(src),
            })
        } else {
            let loc = self.current_loc();
            self.diag
                .add_diagnostic("Expected '\"' at end of string".to_string(), loc);
            None
        }
    }
    fn ident_token(&mut self) -> Option<Token> {
        let mut src = self.next_char()?.to_string();
        while let Some(c) = self.match_char_with(Self::is_ident_char) {
            src.push(c);
        }
        Some(Token {
            loc: self.current_loc(),
            kind: match src.as_str() {
                "fun" => TokenKind::Fun,
                "imm" => TokenKind::Imm,
                "mut" => TokenKind::Mut,
                "borrow" => TokenKind::Borrow,
                "Some" => TokenKind::Some,
                "None" => TokenKind::None,
                "in" => TokenKind::In,
                "for" => TokenKind::For,
                "panic" => TokenKind::Panic,
                "int" => TokenKind::Int,
                "string" => TokenKind::String,
                "bool" => TokenKind::Bool,
                "list" => TokenKind::List,
                "let" => TokenKind::Let,
                "case" => TokenKind::Case,
                "print" => TokenKind::Print,
                "option" => TokenKind::Option,
                "static" => TokenKind::Static,
                "ref" => TokenKind::Ref,
                "box" => TokenKind::Box,
                "char" => TokenKind::Char,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "end" => TokenKind::End,
                "of" => TokenKind::Of,
                "do" => TokenKind::Do,
                _ => TokenKind::Ident(src),
            },
        })
    }
    fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();
        let line = self.line;
        let &c = self.chars.peek()?;
        self.start_line = line;
        match c {
            '.' => Some(self.next_token_from_char(TokenKind::Dot)),
            '=' => Some(self.next_token_from_char_or_chars(
                '>',
                TokenKind::Equal,
                TokenKind::ThickArrow,
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
            _ => {
                self.diag
                    .add_diagnostic(format!("Unrecognised char '{}'", c), self.current_loc());
                Some(self.next_token_from_char(TokenKind::Error))
            }
        }
    }
    pub fn lex(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token() {
            tokens.push(token);
        }
        self.diag.report_all();
        tokens
    }
}
