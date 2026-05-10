use std::{iter::Peekable, vec::IntoIter};

use crate::{
    ast::{
        BinaryOp, CaseArm, Expr, ExprKind, Function, FunctionType, Generics, Ident, IsResource,
        Lambda, LetExpr, Mutable, Param, Pattern, PatternKind, Program, Region, Type, TypeKind,
    },
    diagnostics::DiagnosticReporter,
    parsing::{
        lex::Lexer,
        tokens::{Token, TokenKind},
    },
};
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Precedence {
    None,
    Term,
    Factor,
}
pub struct ParseError;
pub struct Parser {
    diag: DiagnosticReporter,
    last_line: usize,
    tokens: Peekable<IntoIter<Token>>,
}
impl Parser {
    pub fn new(src: &str) -> Self {
        let tokens = Lexer::new(src).lex();
        let last_line = tokens.last().map(|token| token.line).unwrap_or(1);
        Self {
            diag: DiagnosticReporter::new(),
            tokens: tokens.into_iter().peekable(),
            last_line,
        }
    }
    fn current_line(&mut self) -> usize {
        self.peek_token()
            .map(|token| token.line)
            .unwrap_or(self.last_line)
    }
    fn peek_token(&mut self) -> Option<&Token> {
        self.tokens.peek()
    }
    fn next_token(&mut self) -> Option<Token> {
        self.tokens.next()
    }
    fn check_token(&mut self, kind: &TokenKind) -> bool {
        self.peek_token().is_some_and(|token| token.kind == *kind)
    }
    fn check_token_is_ident(&mut self) -> bool {
        self.peek_token()
            .is_some_and(|token| matches!(token.kind, TokenKind::Ident(_)))
    }
    fn match_token(&mut self, kind: &TokenKind) -> Option<Token> {
        let token = self.peek_token()?;
        if token.kind == *kind {
            self.next_token()
        } else {
            None
        }
    }
    fn matches_token(&mut self, kind: &TokenKind) -> bool {
        if self.check_token(kind) {
            self.next_token();
            true
        } else {
            false
        }
    }
    fn match_ident(&mut self) -> Option<Ident> {
        if self.check_token_is_ident() {
            let Token {
                line,
                kind: TokenKind::Ident(name),
            } = self.next_token().expect("There should be a token")
            else {
                unreachable!("Has to be a name")
            };
            Some(Ident {
                content: name,
                line,
            })
        } else {
            None
        }
    }
    fn expect_ident(&mut self, kind: &str) -> Result<Ident, ParseError> {
        if let Some(ident) = self.match_ident() {
            Ok(ident)
        } else {
            let line = self.current_line();
            self.diag.report(format!("Expected '{kind}'"), line);
            Err(ParseError)
        }
    }
    fn expect(&mut self, kind: &TokenKind) -> Result<(), ParseError> {
        let (line, tok) = match self.peek_token() {
            Some(token) => {
                if token.kind == *kind {
                    self.next_token();
                    return Ok(());
                } else {
                    (token.line, Some(&token.kind))
                }
            }
            None => (self.current_line(), None),
        };
        let msg = if let Some(tok) = tok {
            format!("Expected '{}' but got '{}'", kind, tok)
        } else {
            format!("Expected '{}' but got EOF", kind)
        };
        self.diag.report(msg, line);
        Err(ParseError)
    }
    fn expect_error(
        &mut self,
        msg: impl FnOnce(Option<&TokenKind>) -> String,
    ) -> Result<(), ParseError> {
        let (line, kind) = (self.current_line(), self.peek_token().map(|tok| &tok.kind));
        let msg = msg(kind);
        self.diag.report(msg, line);
        Err(ParseError)
    }
    fn binary_op(&mut self) -> Option<(Precedence, BinaryOp)> {
        match self.peek_token()?.kind {
            TokenKind::Plus => Some((Precedence::Factor, BinaryOp::Add)),
            TokenKind::Minus => Some((Precedence::Factor, BinaryOp::Subtract)),
            TokenKind::Slash => Some((Precedence::Term, BinaryOp::Divide)),
            TokenKind::Star => Some((Precedence::Term, BinaryOp::Multiply)),
            _ => None,
        }
    }
    fn parse_binding(&mut self) -> Result<(Mutable, Ident), ParseError> {
        let mutable = if self.matches_token(&TokenKind::Mut) {
            Mutable::Mutable
        } else {
            Mutable::Immutable
        };
        let name = self.expect_ident("variable name")?;
        Ok((mutable, name))
    }
    fn parse_region(&mut self) -> Result<Region, ParseError> {
        match self.match_ident() {
            Some(name) => Ok(Region::Named(name)),
            None => match self.peek_token() {
                Some(&Token {
                    line,
                    kind: TokenKind::Static,
                }) => {
                    self.next_token();
                    Ok(Region::Static(line))
                }
                _ => Err({
                    let line = self.current_line();
                    self.diag
                        .report("Expected a valid region".to_string(), line);
                    ParseError
                }),
            },
        }
    }
    fn parse_pattern_ident(
        &mut self,
        region: Option<Region>,
        line: usize,
        mutable: Mutable,
    ) -> Result<Pattern, ParseError> {
        let name = self.expect_ident("variable name")?;
        Ok(Pattern {
            line,
            kind: PatternKind::Binding(mutable, name, region),
        })
    }
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        match self.peek_token() {
            None => {
                let line = self.current_line();
                self.diag.report("Expected a pattern".to_string(), line);
                Err(ParseError)
            }
            Some(&Token { line, ref kind }) => match kind {
                TokenKind::Ref => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    let region = self.parse_region()?;
                    let _ = self.expect(&TokenKind::RightBracket);

                    let mutable = if self.matches_token(&TokenKind::Mut) {
                        Mutable::Mutable
                    } else {
                        Mutable::Immutable
                    };
                    self.parse_pattern_ident(Some(region), line, mutable)
                }
                TokenKind::Ident(_) => self.parse_pattern_ident(None, line, Mutable::Immutable),
                TokenKind::Mut => {
                    self.next_token();
                    self.parse_pattern_ident(None, line, Mutable::Mutable)
                }
                TokenKind::Some => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let pat = self.parse_pattern()?;
                    let _ = self.expect(&TokenKind::RightParen);
                    Ok(Pattern {
                        line,
                        kind: PatternKind::Some(Box::new(pat)),
                    })
                }
                TokenKind::None => {
                    self.next_token();
                    Ok(Pattern {
                        line,
                        kind: PatternKind::None,
                    })
                }
                TokenKind::True => {
                    self.next_token();
                    Ok(Pattern {
                        line,
                        kind: PatternKind::Bool(true),
                    })
                }
                TokenKind::False => {
                    self.next_token();
                    Ok(Pattern {
                        line,
                        kind: PatternKind::Bool(false),
                    })
                }
                TokenKind::Caret => {
                    self.next_token();
                    let pattern = self.parse_pattern()?;
                    Ok(Pattern {
                        line,
                        kind: PatternKind::Deref(Box::new(pattern)),
                    })
                }
                _ => {
                    self.diag
                        .report("Expected a valid pattern".to_string(), line);
                    Err(ParseError)
                }
            },
        }
    }
    fn parse_case_arm(&mut self) -> Result<CaseArm, ParseError> {
        let pattern = self.parse_pattern()?;
        let _ = self.expect(&TokenKind::Arrow);
        let body = self.parse_expr()?;
        Ok(CaseArm { pat: pattern, body })
    }
    fn parse_expr_prefix(&mut self) -> Result<Expr, ParseError> {
        match self.peek_token() {
            None => {
                let line = self.current_line();
                self.diag.report("Expected expr".to_string(), line);
                Err(ParseError)
            }
            Some(token @ &Token { line, .. }) => match token.kind {
                TokenKind::Number(num) => {
                    self.next_token();
                    Ok(Expr {
                        line,
                        kind: ExprKind::Number(num),
                    })
                }
                TokenKind::True => {
                    self.next_token();
                    Ok(Expr {
                        line,
                        kind: ExprKind::Bool(true),
                    })
                }
                TokenKind::False => {
                    self.next_token();
                    Ok(Expr {
                        line,
                        kind: ExprKind::Bool(false),
                    })
                }
                TokenKind::LeftParen => {
                    self.next_token();
                    let expr = if self.check_token(&TokenKind::RightParen) {
                        Expr {
                            line,
                            kind: ExprKind::Unit,
                        }
                    } else {
                        self.parse_expr()?
                    };
                    let _ = self.expect(&TokenKind::RightParen);
                    Ok(expr)
                }
                TokenKind::For => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let pattern = self.parse_pattern()?;
                    let _ = self.expect(&TokenKind::In);
                    let iterator = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::RightParen);
                    let _ = self.expect(&TokenKind::LeftBrace);
                    let body = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::RightBrace);
                    Ok(Expr {
                        line,
                        kind: ExprKind::For(pattern, Box::new(iterator), Box::new(body)),
                    })
                }
                TokenKind::Borrow => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let (mutable, name) = self.parse_binding()?;
                    let _ = self.expect(&TokenKind::In);
                    let region = self.expect_ident("region name")?;
                    let _ = self.expect(&TokenKind::RightParen);
                    let _ = self.expect(&TokenKind::LeftBrace);
                    let body = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::RightBrace);
                    Ok(Expr {
                        line,
                        kind: ExprKind::Borrow(mutable, name, region, Box::new(body)),
                    })
                }
                TokenKind::Let => {
                    self.next_token();
                    let pattern = self.parse_pattern()?;
                    let ty = if self.matches_token(&TokenKind::Colon) {
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    let _ = self.expect(&TokenKind::Equal);
                    let expr = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::In);
                    let body = self.parse_expr()?;
                    Ok(Expr {
                        line,
                        kind: ExprKind::Let(Box::new(LetExpr {
                            pattern,
                            ty,
                            binder: expr,
                            body,
                        })),
                    })
                }
                TokenKind::Ident(_) => {
                    let Some(name) = self.match_ident() else {
                        unreachable!("Should be an ident on {line}")
                    };
                    Ok(Expr {
                        line,
                        kind: ExprKind::Ident(name),
                    })
                }
                TokenKind::Some => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let expr = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::RightParen);

                    Ok(Expr {
                        line,
                        kind: ExprKind::Some(Box::new(expr)),
                    })
                }
                TokenKind::None => {
                    self.next_token();
                    let ty = if self.matches_token(&TokenKind::LeftBracket) {
                        let ty = self.parse_type()?;
                        let _ = self.expect(&TokenKind::RightBracket);
                        Some(ty)
                    } else {
                        None
                    };
                    Ok(Expr {
                        line,
                        kind: ExprKind::None(ty),
                    })
                }
                TokenKind::Print => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let expr = if self.check_token(&TokenKind::RightParen) {
                        None
                    } else {
                        let expr = self.parse_expr()?;
                        Some(expr)
                    };
                    let _ = self.expect(&TokenKind::RightParen);

                    Ok(Expr {
                        line,
                        kind: ExprKind::Print(expr.map(Box::new)),
                    })
                }
                TokenKind::Panic => {
                    self.next_token();
                    let ty = if self.matches_token(&TokenKind::LeftBracket) {
                        let ty = self.parse_type()?;
                        let _ = self.expect(&TokenKind::RightBracket);
                        Some(ty)
                    } else {
                        None
                    };
                    Ok(Expr {
                        line,
                        kind: ExprKind::Panic(ty),
                    })
                }
                TokenKind::Case => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let matchee = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::RightParen);
                    let _ = self.expect(&TokenKind::LeftBrace);
                    let mut arms = Vec::new();
                    while !self.check_token(&TokenKind::RightBrace) {
                        arms.push(self.parse_case_arm()?);
                        if self.match_token(&TokenKind::Coma).is_none() {
                            break;
                        }
                    }
                    let _ = self.expect(&TokenKind::RightBrace);
                    Ok(Expr {
                        line,
                        kind: ExprKind::Case(Box::new(matchee), arms),
                    })
                }
                TokenKind::StringLiteral(_) => {
                    let Some(Token {
                        line,
                        kind: TokenKind::StringLiteral(string),
                    }) = self.next_token()
                    else {
                        unreachable!("Should be a string literal here {line}")
                    };
                    Ok(Expr {
                        line,
                        kind: ExprKind::String(string),
                    })
                }
                TokenKind::List => {
                    self.next_token();
                    let mut values = Vec::new();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    while !self.check_token(&TokenKind::RightBracket) {
                        values.push(self.parse_expr()?);
                        if !self.matches_token(&TokenKind::Coma) {
                            break;
                        }
                    }
                    let _ = self.expect(&TokenKind::RightBracket);
                    Ok(Expr {
                        line,
                        kind: ExprKind::List(values),
                    })
                }
                TokenKind::Fun => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let mut params = Vec::new();
                    while let Some(name) = self.match_ident() {
                        let param_type = if self.matches_token(&TokenKind::Colon) {
                            Some(self.parse_type()?)
                        } else {
                            None
                        };
                        params.push((name, param_type));
                        if self.match_token(&TokenKind::Coma).is_none() {
                            break;
                        }
                    }
                    let _ = self.expect(&TokenKind::RightParen);

                    let resource = if self.matches_token(&TokenKind::Arrow) {
                        IsResource::Data
                    } else if self.matches_token(&TokenKind::ThickArrow) {
                        IsResource::Resource
                    } else {
                        let _ = self.expect_error(|msg| match msg {
                            Some(kind) => format!("Expected '->' or '=>' but got '{kind}'"),
                            None => "Expected '->' or '=>' but got EOF".to_string(),
                        });
                        IsResource::Data
                    };
                    let body = self.parse_expr()?;
                    Ok(Expr {
                        line,
                        kind: ExprKind::Lambda(Lambda {
                            params,
                            resource,
                            body: Box::new(body),
                        }),
                    })
                }
                ref kind => {
                    let msg = format!("Expected valid expr but got {kind:?}");
                    let line = self.current_line();
                    self.diag.report(msg, line);
                    Err(ParseError)
                }
            },
        }
    }
    fn parse_expr_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_expr_prefix()?;
        loop {
            expr = match self.peek_token() {
                None => break Ok(expr),
                Some(token) => match token.kind {
                    TokenKind::LeftParen => {
                        self.next_token();
                        let mut args = Vec::new();
                        while self
                            .peek_token()
                            .is_some_and(|token| !matches!(token.kind, TokenKind::RightParen))
                        {
                            args.push(self.parse_expr()?);
                            if self.match_token(&TokenKind::Coma).is_none() {
                                break;
                            }
                        }
                        let _ = self.expect(&TokenKind::RightParen);
                        Expr {
                            line: expr.line,
                            kind: ExprKind::Call(Box::new(expr), args),
                        }
                    }
                    TokenKind::Caret => {
                        self.next_token();
                        Expr {
                            line: expr.line,
                            kind: ExprKind::Deref(Box::new(expr)),
                        }
                    }
                    TokenKind::Colon => {
                        self.next_token();
                        let ty = self.parse_type()?;
                        Expr {
                            line: expr.line,
                            kind: ExprKind::Annotate(Box::new(expr), Box::new(ty)),
                        }
                    }
                    _ => break Ok(expr),
                },
            }
        }
    }
    fn parse_expr_precedence(&mut self, prec: Precedence) -> Result<Expr, ParseError> {
        let mut expr = self.parse_expr_postfix()?;
        while let Some((curr_prec, op)) = self.binary_op()
            && curr_prec >= prec
        {
            self.next_token();
            let rhs = self.parse_expr_precedence(prec)?;
            expr = Expr {
                line: expr.line,
                kind: ExprKind::Binary(op, Box::new(expr), Box::new(rhs)),
            }
        }
        Ok(expr)
    }
    fn parse_assign(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_expr_precedence(Precedence::None)?;
        while self.matches_token(&TokenKind::Equal) {
            let line = lhs.line;
            lhs = match lhs.as_place() {
                Ok(place) => Expr {
                    line,
                    kind: ExprKind::Assign(
                        place,
                        Box::new(self.parse_expr_precedence(Precedence::None)?),
                    ),
                },
                Err(non_place) => {
                    lhs = non_place;
                    self.diag
                        .report("Invalid assignment target".to_string(), line);
                    break;
                }
            };
        }
        Ok(lhs)
    }
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let mut first = self.parse_assign()?;
        while self.matches_token(&TokenKind::Semi) {
            first = Expr {
                line: first.line,
                kind: ExprKind::Sequence(Box::new(first), Box::new(self.parse_assign()?)),
            };
        }
        Ok(first)
    }
    fn parse_optional_generics(&mut self) -> Result<Option<Generics>, ParseError> {
        if let Some(Token { line, .. }) = self.match_token(&TokenKind::LeftBracket) {
            let mut names = Vec::new();
            while let Some(name) = self.match_ident() {
                names.push(name);
                if self.match_token(&TokenKind::Coma).is_none() {
                    break;
                }
            }
            let _ = self.expect(&TokenKind::RightBracket);
            Ok(Some(Generics { line, names }))
        } else {
            Ok(None)
        }
    }
    fn parse_type_function(&mut self) -> Result<FunctionType, ParseError> {
        let _ = self.expect(&TokenKind::Fun);
        let _ = self.expect(&TokenKind::LeftParen);
        let mut params = Vec::new();
        while self
            .peek_token()
            .is_some_and(|token| !matches!(token.kind, TokenKind::RightParen))
        {
            params.push(self.parse_type()?);
            if self.match_token(&TokenKind::Coma).is_none() {
                break;
            }
        }
        let _ = self.expect(&TokenKind::RightParen);
        let is_resource = if self.matches_token(&TokenKind::Arrow) {
            IsResource::Resource
        } else if self.matches_token(&TokenKind::ThickArrow) {
            IsResource::Data
        } else {
            let _ = self.expect_error(|kind| match kind {
                Some(kind) => format!("Expected '->' or '=>' but got '{kind}'"),
                None => format!("Expected '->' or '=>' but got 'EOF'"),
            });
            IsResource::Data
        };

        let return_type = self.parse_type()?;
        Ok(FunctionType {
            resource: is_resource,
            params,
            return_type: Box::new(return_type),
        })
    }
    fn parse_optional_type(&mut self) -> Result<Option<Type>, ParseError> {
        match self.peek_token() {
            None => Ok(None),
            Some(&Token { line, ref kind }) => match kind {
                TokenKind::Mut => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    let region = self.parse_region()?;
                    let _ = self.expect(&TokenKind::RightBracket);
                    let ty = self.parse_type()?;
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Mut(region, Box::new(ty)),
                    }))
                }
                TokenKind::Imm => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    let region = self.parse_region()?;
                    let _ = self.expect(&TokenKind::RightBracket);
                    let ty = self.parse_type()?;
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Imm(region, Box::new(ty)),
                    }))
                }
                TokenKind::Int => {
                    self.next_token();
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Int,
                    }))
                }
                TokenKind::Bool => {
                    self.next_token();
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Bool,
                    }))
                }
                TokenKind::String => {
                    self.next_token();
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::String,
                    }))
                }
                TokenKind::List => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    let ty = self.parse_type()?;
                    let _ = self.expect(&TokenKind::RightBracket);
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::List(Box::new(ty)),
                    }))
                }
                TokenKind::LeftParen => {
                    self.next_token();
                    if self.matches_token(&TokenKind::RightParen) {
                        Ok(Some(Type {
                            line,
                            kind: TypeKind::Unit,
                        }))
                    } else {
                        let ty = self.parse_type()?;
                        let _ = self.expect(&TokenKind::RightParen);
                        Ok(Some(ty))
                    }
                }
                TokenKind::Option => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    let ty = self.parse_type()?;
                    let _ = self.expect(&TokenKind::RightBracket);
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Option(Box::new(ty)),
                    }))
                }
                TokenKind::Ident(_) => {
                    let name = self.match_ident().expect("Expected valid ident");
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Named(name),
                    }))
                }
                TokenKind::Fun => {
                    let function = self.parse_type_function()?;
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Function(function),
                    }))
                }
                TokenKind::Box => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftBracket);
                    let ty = self.parse_type()?;
                    let _ = self.expect(&TokenKind::RightBracket);
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Box(Box::new(ty)),
                    }))
                }
                TokenKind::Char => {
                    self.next_token();
                    Ok(Some(Type {
                        line,
                        kind: TypeKind::Char,
                    }))
                }
                TokenKind::Forall => {
                    self.next_token();
                    let generics = self.parse_optional_generics()?;
                    let function_type = self.parse_type_function()?;
                    let Some(_generics) = generics else {
                        self.diag.report(format!("Expected regions"), line);
                        return Err(ParseError);
                    };
                    Ok(Some(Type { line, kind: TypeKind::Function(function_type) }))
                }
                _ => Ok(None),
            },
        }
    }
    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let ty = self.parse_optional_type()?;
        match ty {
            Some(ty) => Ok(ty),
            None => Err({
                let line = self.current_line();
                let msg = if let Some(kind) = self.peek_token().map(|token| &token.kind) {
                    format!("Expected a type but got '{kind}'",)
                } else {
                    "Expected a type but got eof".to_string()
                };
                self.diag.report(msg, line);
                ParseError
            }),
        }
    }
    fn parse_param(&mut self) -> Result<Param, ParseError> {
        let name = self.expect_ident("param name")?;
        let _ = self.expect(&TokenKind::Colon);
        let ty = self.parse_type()?;
        Ok(Param { name, ty })
    }
    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let line = self.current_line();
        let _ = self.expect(&TokenKind::Fun);
        let name = self.expect_ident("function name")?;
        let generics = self.parse_optional_generics()?;
        let _ = self.expect(&TokenKind::LeftParen);
        let mut params = Vec::new();
        while self.check_token_is_ident() {
            let param = self.parse_param()?;
            params.push(param);
            if self.match_token(&TokenKind::Coma).is_none() {
                break;
            }
        }
        let _ = self.expect(&TokenKind::RightParen);
        let _ = self.expect(&TokenKind::Arrow);
        let ty = self.parse_type()?;
        let _ = self.expect(&TokenKind::Equal);
        let body = self.parse_expr()?;
        Ok(Function {
            line,
            name,
            generics,
            params,
            return_type: ty,
            body,
        })
    }
    pub fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut functions = Vec::new();
        while self.peek_token().is_some() {
            let Ok(function) = self.parse_function() else {
                while let Some(token) = self.peek_token()
                    && !matches!(token.kind, TokenKind::Fun)
                {
                    self.next_token();
                }
                continue;
            };
            functions.push(function);
        }
        self.diag.finish();
        Ok(Program { functions })
    }
}
