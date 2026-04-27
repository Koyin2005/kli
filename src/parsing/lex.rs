use std::{iter::Peekable, num::IntErrorKind, str::Chars};

use crate::{
    diagnostics::DiagnosticReporter,
    parsing::tokens::{Token, TokenKind},
};

pub struct Lexer<'src> {
    chars: Peekable<Chars<'src>>,
    line: usize,
    diag: DiagnosticReporter,
}
impl<'s> Lexer<'s> {
    pub fn new(src: &'s str) -> Self {
        Self {
            chars: src.chars().peekable(),
            line: 1,
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
    fn new_token_from_char(&mut self, line: usize, kind: TokenKind) -> Token {
        self.next_char();
        Token { line, kind }
    }
    fn new_token_from_char_or_chars(
        &mut self,
        next_char: char,
        line: usize,
        single_kind: TokenKind,
        double_kind: TokenKind,
    ) -> Token {
        self.next_char();
        if self.match_char(next_char).is_some() {
            Token {
                line,
                kind: double_kind,
            }
        } else {
            Token {
                line,
                kind: single_kind,
            }
        }
    }
    fn is_start_char(c: char) -> bool {
        c.is_ascii_alphabetic() || c == '_'
    }
    fn is_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }
    fn num_token(&mut self, line: usize) -> Option<Token> {
        let mut src = String::new();
        while let Some(c) = self.match_char_with(|c| char::is_digit(c, 10)) {
            src.push(c);
        }
        match src.parse::<u64>() {
            Ok(n) => Some(Token {
                line,
                kind: TokenKind::Number(n),
            }),
            Err(e) => match e.kind() {
                IntErrorKind::PosOverflow => {
                    self.diag.report("Integer too large".to_string(), line);
                    Some(Token {
                        line,
                        kind: TokenKind::Number(u64::MAX),
                    })
                }
                _ => None,
            },
        }
    }
    fn string_token(&mut self, line: usize) -> Option<Token> {
        self.next_char();
        let mut src = String::new();
        while let Some(c) = self.peek_char()
            && c != '"'
        {
            src.push(c);
        }

        if self.match_char('"').is_some() {
            Some(Token {
                line,
                kind: TokenKind::StringLiteral(src),
            })
        } else {
            self.diag
                .report("Expected '\"' at end of string".to_string(), line);
            None
        }
    }
    fn ident_token(&mut self, line: usize) -> Option<Token> {
        let mut src = self.next_char()?.to_string();
        while let Some(c) = self.match_char_with(Self::is_ident_char) {
            src.push(c);
        }
        Some(Token {
            line,
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
                _ => TokenKind::Ident(src),
            },
        })
    }
    fn next_token(&mut self) -> Option<Token> {
        self.skip_whitespace();
        let line = self.line;
        let &c = self.chars.peek()?;
        match c {
            '=' => Some(self.new_token_from_char(line, TokenKind::Equal)),
            '(' => Some(self.new_token_from_char(line, TokenKind::LeftParen)),
            ')' => Some(self.new_token_from_char(line, TokenKind::RightParen)),
            '{' => Some(self.new_token_from_char(line, TokenKind::LeftBrace)),
            '}' => Some(self.new_token_from_char(line, TokenKind::RightBrace)),
            '[' => Some(self.new_token_from_char(line, TokenKind::LeftBracket)),
            ']' => Some(self.new_token_from_char(line, TokenKind::RightBracket)),
            '+' => Some(self.new_token_from_char(line, TokenKind::Plus)),
            '-' => Some(self.new_token_from_char_or_chars(
                '>',
                line,
                TokenKind::Minus,
                TokenKind::Arrow,
            )),
            '/' => Some(self.new_token_from_char(line, TokenKind::Slash)),
            '*' => Some(self.new_token_from_char(line, TokenKind::Star)),
            ',' => Some(self.new_token_from_char(line, TokenKind::Coma)),
            ';' => Some(self.new_token_from_char(line, TokenKind::Semi)),
            ':' => Some(self.new_token_from_char(line, TokenKind::Colon)),
            '^' => Some(self.new_token_from_char(line, TokenKind::Caret)),
            c if Self::is_start_char(c) => self.ident_token(line),
            c if c.is_numeric() => self.num_token(line),
            '"' => self.string_token(line),
            _ => {
                self.diag.report(format!("Unrecognised char '{}'", c), line);
                Some(self.new_token_from_char(line, TokenKind::Error))
            }
        }
    }
    pub fn lex(mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        while let Some(token) = self.next_token() {
            tokens.push(token);
        }
        self.diag.finish();
        tokens
    }
}
