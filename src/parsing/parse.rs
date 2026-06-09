use std::{iter::Peekable, rc::Rc, vec::IntoIter};

use crate::{
    ast::{
        Annotation, AnnotationField, BinaryOp, BlockBody, BorrowExpr, CaseArm, Expr, ExprKind,
        FieldInit, Function, FunctionType, Generics, IsResource, Lambda, LetBinding, Module,
        Mutable, Param, Path, Pattern, PatternField, PatternKind, RecordExpr, RecordField,
        RecordType, Region, Stmt, StmtKind, Type, TypeKind,
    },
    diagnostics::DiagnosticReporter,
    ident::Ident,
    parsing::{
        lex::Lexer,
        tokens::{Token, TokenKind},
    },
    src_loc::SrcLoc,
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
    file: Rc<str>,
    tokens: Peekable<IntoIter<Token>>,
}
impl Parser {
    pub fn new(file: Rc<str>, src: &str) -> Self {
        let tokens = Lexer::new(file.clone(), src).lex();
        Self {
            diag: DiagnosticReporter::new(),
            file,
            tokens: tokens.into_iter().peekable(),
        }
    }
    fn current_loc(&mut self) -> SrcLoc {
        self.peek_token()
            .map(|token| token.loc.clone())
            .unwrap_or(SrcLoc {
                line: 1,
                file: self.file.clone(),
            })
    }
    fn peek_token(&mut self) -> Option<&Token> {
        self.tokens.peek()
    }
    fn next_token(&mut self) -> Option<Token> {
        self.tokens.next()
    }
    fn check_token(&mut self, kind: &TokenKind) -> bool {
        let Some(token) = self.peek_token() else {
            return false;
        };
        token.kind == *kind
    }
    fn check_is_not_token(&mut self, kind: &TokenKind) -> bool {
        let Some(token) = self.peek_token() else {
            return false;
        };
        token.kind != *kind
    }
    fn check_token_is_ident(&mut self) -> bool {
        let Some(token) = self.peek_token() else {
            return false;
        };
        matches!(token.kind, TokenKind::Ident(_))
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
    fn not_matches_token(&mut self, kind: &TokenKind) -> bool {
        if self.check_token(kind) {
            self.next_token();
            false
        } else {
            true
        }
    }
    fn match_ident(&mut self) -> Option<Ident> {
        if self.check_token_is_ident() {
            let Token {
                loc,
                kind: TokenKind::Ident(name),
            } = self.next_token().expect("There should be a token")
            else {
                unreachable!("Has to be a name")
            };
            Some(Ident {
                content: name.into(),
                loc,
            })
        } else {
            None
        }
    }
    fn expect_ident(&mut self, kind: &str) -> Result<Ident, ParseError> {
        if let Some(ident) = self.match_ident() {
            Ok(ident)
        } else {
            let loc = self.current_loc();
            let msg = if let Some(token) = self.peek_token() {
                format!("Expected '{kind}' but got '{}'", token.kind)
            } else {
                format!("Expected '{kind}' but got 'EOF'")
            };
            self.diag.add_diagnostic(msg, loc);
            Err(ParseError)
        }
    }
    fn match_string_literal(&mut self) -> Option<(SrcLoc, String)> {
        if self.peek_token().is_some_and(|token| {
            matches!(
                token,
                Token {
                    loc: _,
                    kind: TokenKind::StringLiteral(_)
                }
            )
        }) {
            let Token {
                loc,
                kind: TokenKind::StringLiteral(value),
            } = self.next_token().expect("Should be a string literal")
            else {
                unreachable!("Should be a string literal")
            };
            Some((loc, value))
        } else {
            None
        }
    }
    fn expect(&mut self, kind: &TokenKind) -> Result<(), ParseError> {
        let (loc, tok) = match self.peek_token() {
            Some(token) => {
                if token.kind == *kind {
                    self.next_token();
                    return Ok(());
                } else {
                    (token.loc.clone(), Some(&token.kind))
                }
            }
            None => (self.current_loc(), None),
        };
        let msg = if let Some(tok) = tok {
            format!("Expected '{}' but got '{}'", kind, tok)
        } else {
            format!("Expected '{}' but got EOF", kind)
        };
        self.diag.add_diagnostic(msg, loc);
        Err(ParseError)
    }
    fn expect_error(
        &mut self,
        msg: impl FnOnce(Option<&TokenKind>) -> String,
    ) -> Result<(), ParseError> {
        let (loc, kind) = (self.current_loc(), self.peek_token().map(|tok| &tok.kind));
        let msg = msg(kind);
        self.diag.add_diagnostic(msg, loc);
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
    fn parse_region(&mut self) -> Result<Region, ParseError> {
        match self.match_ident() {
            Some(name) => Ok(Region::Named(name)),
            None => match self.match_token(&TokenKind::Static) {
                Some(Token {
                    loc,
                    kind: TokenKind::Static,
                }) => {
                    let loc = loc.clone();
                    self.next_token();
                    Ok(Region::Static(loc.clone()))
                }
                _ => Err({
                    let loc = self.current_loc();
                    self.diag
                        .add_diagnostic("Expected a valid region".to_string(), loc);
                    ParseError
                }),
            },
        }
    }
    fn parse_pattern_ident(
        &mut self,
        borrow: Option<Mutable>,
        loc: SrcLoc,
        mutable: Mutable,
    ) -> Result<Pattern, ParseError> {
        let name = self.expect_ident("variable name")?;
        Ok(Pattern {
            loc,
            kind: PatternKind::Binding(borrow, mutable, name),
        })
    }
    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let loc = self.current_loc();
        match self.peek_token() {
            None => {
                self.diag
                    .add_diagnostic("Expected a pattern".to_string(), loc);
                Err(ParseError)
            }
            Some(Token { loc: _, kind }) => match kind {
                &TokenKind::Number(number) => {
                    self.next_token();
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::Int(number),
                    })
                }
                TokenKind::Ref => {
                    self.next_token();
                    let pattern = self.parse_pattern()?;
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::Ref(Box::new(pattern)),
                    })
                }
                TokenKind::Borrow => {
                    self.next_token();
                    let mutable = if self.matches_token(&TokenKind::Mut) {
                        Mutable::Mutable
                    } else {
                        Mutable::Immutable
                    };
                    self.parse_pattern_ident(Some(mutable), loc, Mutable::Immutable)
                }
                TokenKind::Ident(_) => self.parse_pattern_ident(None, loc, Mutable::Immutable),
                TokenKind::Mut => {
                    self.next_token();
                    self.parse_pattern_ident(None, loc, Mutable::Mutable)
                }
                TokenKind::Some => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let pat = self.parse_pattern()?;
                    let _ = self.expect(&TokenKind::RightParen);
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::Some(Box::new(pat)),
                    })
                }
                TokenKind::None => {
                    self.next_token();
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::None,
                    })
                }
                TokenKind::True => {
                    self.next_token();
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::Bool(true),
                    })
                }
                TokenKind::False => {
                    self.next_token();
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::Bool(false),
                    })
                }
                TokenKind::LeftBrace => {
                    self.next_token();
                    let mut fields = Vec::new();
                    while self.check_is_not_token(&TokenKind::RightBrace) {
                        let name = self.expect_ident("field name")?;
                        let _ = self.expect(&TokenKind::Equal);
                        let pattern = self.parse_pattern()?;
                        fields.push(PatternField { name, pattern });
                        if self.not_matches_token(&TokenKind::Coma) {
                            break;
                        }
                    }
                    let _ = self.expect(&TokenKind::RightBrace);
                    Ok(Pattern {
                        loc,
                        kind: PatternKind::Record(fields),
                    })
                }
                _ => {
                    self.diag
                        .add_diagnostic("Expected a valid pattern".to_string(), loc);
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
    fn parse_resource_arrow(&mut self) -> Result<IsResource, ParseError> {
        if self.matches_token(&TokenKind::Arrow) {
            Ok(IsResource::Data)
        } else if self.matches_token(&TokenKind::ThickArrow) {
            Ok(IsResource::Resource)
        } else {
            let _ = self.expect_error(|msg| match msg {
                Some(kind) => format!("Expected '->' or '=>' but got '{kind}'"),
                None => "Expected '->' or '=>' but got EOF".to_string(),
            });
            Err(ParseError)
        }
    }
    fn parse_block_body(&mut self) -> Result<BlockBody, ParseError> {
        let mut stmts = Vec::new();
        loop {
            let stmt = if let Some(stmt) = self.parse_definition_stmt()? {
                stmt
            } else {
                let expr = self.parse_expr()?;
                if !self.matches_token(&TokenKind::Semi) {
                    break Ok(BlockBody {
                        stmts,
                        expr: Box::new(expr),
                    });
                }
                Stmt {
                    loc: expr.loc.clone(),
                    kind: StmtKind::Expr(expr),
                }
            };
            stmts.push(stmt);
        }
    }
    fn parse_block_expr(&mut self, loc: SrcLoc) -> Result<Expr, ParseError> {
        self.expect(&TokenKind::Do)?;
        let region = if self.matches_token(&TokenKind::In) {
            Some(self.expect_ident("region name")?)
        } else {
            None
        };
        let body = self.parse_block_body()?;
        self.expect(&TokenKind::End)?;
        Ok(Expr {
            loc,
            kind: ExprKind::Block(body, region),
        })
    }
    fn parse_case_expr(&mut self, loc: SrcLoc) -> Result<Expr, ParseError> {
        self.next_token();
        let matchee = self.parse_expr()?;
        let _ = self.expect(&TokenKind::Of);
        let mut arms = Vec::new();
        while self.matches_token(&TokenKind::Pipe) {
            arms.push(self.parse_case_arm()?);
        }
        let _ = self.expect(&TokenKind::End);
        Ok(Expr {
            loc,
            kind: ExprKind::Case(Box::new(matchee), arms),
        })
    }
    fn parse_definition_stmt(&mut self) -> Result<Option<Stmt>, ParseError> {
        let Some(Token { loc, kind }) = self.peek_token() else {
            return Ok(None);
        };
        match kind {
            TokenKind::Let => {
                let loc = loc.clone();
                self.parse_let_stmt(loc).map(Some)
            }
            _ => Ok(None),
        }
    }
    fn parse_let_binding(&mut self) -> Result<LetBinding, ParseError> {
        self.next_token();
        let pattern = self.parse_pattern()?;
        let ty = if self.matches_token(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        let _ = self.expect(&TokenKind::Equal);
        let expr = self.parse_single_expr()?;
        let _ = self.expect(&TokenKind::Semi);
        Ok(LetBinding {
            pattern,
            ty,
            value: expr,
        })
    }
    fn parse_let_stmt(&mut self, loc: SrcLoc) -> Result<Stmt, ParseError> {
        let binding = self.parse_let_binding()?;
        Ok(Stmt {
            loc,
            kind: StmtKind::Let(binding),
        })
    }
    fn parse_record_expr(&mut self, loc: SrcLoc) -> Result<Expr, ParseError> {
        self.next_token();
        let mut fields = Vec::new();
        while self.check_is_not_token(&TokenKind::RightBrace) {
            let name = self.expect_ident("field name")?;
            let _ = self.expect(&TokenKind::Equal);
            let value = self.parse_expr()?;
            fields.push(FieldInit { name, value });
            if self.not_matches_token(&TokenKind::Coma) {
                break;
            }
        }
        self.expect(&TokenKind::RightBrace)?;
        Ok(Expr {
            loc,
            kind: ExprKind::Record(RecordExpr { fields }),
        })
    }
    fn parse_paren_expr(&mut self, loc: SrcLoc) -> Result<Expr, ParseError> {
        self.next_token();
        if self.check_token(&TokenKind::RightParen) {
            self.next_token();
            return Ok(Expr {
                loc,
                kind: ExprKind::Unit,
            });
        }
        let expr = {
            let mut expr = self.parse_expr()?;
            if self.matches_token(&TokenKind::Colon) {
                let ty = self.parse_type()?;
                expr = Expr {
                    loc,
                    kind: ExprKind::Annotate(Box::new(expr), Box::new(ty)),
                };
            };
            expr
        };
        let _ = self.expect(&TokenKind::RightParen);
        Ok(expr)
    }
    fn parse_expr_prefix(&mut self) -> Result<Expr, ParseError> {
        let loc = self.current_loc();
        match self.peek_token() {
            None => {
                self.diag.add_diagnostic("Expected expr".to_string(), loc);
                Err(ParseError)
            }
            Some(token) => match token.kind {
                TokenKind::Number(num) => {
                    self.next_token();
                    Ok(Expr {
                        loc,
                        kind: ExprKind::Number(num),
                    })
                }
                TokenKind::True => {
                    self.next_token();
                    Ok(Expr {
                        loc,
                        kind: ExprKind::Bool(true),
                    })
                }
                TokenKind::False => {
                    self.next_token();
                    Ok(Expr {
                        loc,
                        kind: ExprKind::Bool(false),
                    })
                }
                TokenKind::LeftParen => self.parse_paren_expr(loc),
                TokenKind::For => {
                    self.next_token();
                    let pattern = self.parse_pattern()?;
                    let _ = self.expect(&TokenKind::In);
                    let iterator = self.parse_expr()?;
                    let body = {
                        let loc = self.current_loc();
                        self.parse_block_expr(loc)?
                    };
                    Ok(Expr {
                        loc,
                        kind: ExprKind::For(Box::new(pattern), Box::new(iterator), Box::new(body)),
                    })
                }
                TokenKind::Borrow => {
                    self.next_token();
                    let mutable = if self.matches_token(&TokenKind::Mut) {
                        Mutable::Mutable
                    } else {
                        Mutable::Immutable
                    };
                    let expr = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::In);
                    let region = self.parse_region()?;
                    Ok(Expr {
                        loc,
                        kind: ExprKind::Borrow(Box::new(BorrowExpr {
                            mutable,
                            expr,
                            region,
                        })),
                    })
                }
                TokenKind::Ident(_) => {
                    let Some(name) = self.match_ident() else {
                        unreachable!("Should be an ident here")
                    };
                    let mut path = vec![name];
                    while self.matches_token(&TokenKind::Dot) {
                        let name = self.expect_ident("field name or sub path")?;
                        path.push(name);
                    }
                    Ok(Expr {
                        loc,
                        kind: ExprKind::Path(Path::new(path)),
                    })
                }
                TokenKind::Some => {
                    self.next_token();
                    let _ = self.expect(&TokenKind::LeftParen);
                    let expr = self.parse_expr()?;
                    let _ = self.expect(&TokenKind::RightParen);

                    Ok(Expr {
                        loc,
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
                        loc,
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
                        loc,
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
                        loc,
                        kind: ExprKind::Panic(ty),
                    })
                }
                TokenKind::Case => self.parse_case_expr(loc),
                TokenKind::Do => self.parse_block_expr(loc),
                TokenKind::StringLiteral(_) => {
                    let Some(Token {
                        loc,
                        kind: TokenKind::StringLiteral(string),
                    }) = self.next_token()
                    else {
                        unreachable!("Should be a string literal here")
                    };
                    Ok(Expr {
                        loc,
                        kind: ExprKind::String(string),
                    })
                }
                TokenKind::ArrayList => {
                    self.next_token();
                    self.expect(&TokenKind::LeftBracket)?;
                    let mut values = Vec::new();
                    while self.check_is_not_token(&TokenKind::RightBracket) {
                        values.push(self.parse_expr()?);
                        if self.not_matches_token(&TokenKind::Coma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RightBracket)?;
                    Ok(Expr {
                        loc,
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
                        if self.not_matches_token(&TokenKind::Coma) {
                            break;
                        }
                    }
                    let _ = self.expect(&TokenKind::RightParen);

                    let resource = self.parse_resource_arrow()?;
                    let body = self.parse_expr()?;
                    Ok(Expr {
                        loc,
                        kind: ExprKind::Lambda(Box::new(Lambda {
                            params,
                            resource,
                            body: Box::new(body),
                        })),
                    })
                }
                TokenKind::LeftBrace => self.parse_record_expr(loc),
                ref kind => {
                    let msg = format!("Expected valid expr but got {kind}");
                    let loc = self.current_loc();
                    self.diag.add_diagnostic(msg, loc);
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
                        while self.check_is_not_token(&TokenKind::RightParen) {
                            args.push(self.parse_expr()?);
                            if self.not_matches_token(&TokenKind::Coma) {
                                break;
                            }
                        }
                        let _ = self.expect(&TokenKind::RightParen);
                        Expr {
                            loc: expr.loc.clone(),
                            kind: ExprKind::Call(Box::new(expr), args),
                        }
                    }
                    TokenKind::Caret => {
                        self.next_token();
                        Expr {
                            loc: expr.loc.clone(),
                            kind: ExprKind::Deref(Box::new(expr)),
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
                loc: expr.loc.clone(),
                kind: ExprKind::Binary(op, Box::new(expr), Box::new(rhs)),
            }
        }
        Ok(expr)
    }
    fn parse_assign(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_expr_precedence(Precedence::None)?;
        while self.matches_token(&TokenKind::Equal) {
            let loc = lhs.loc.clone();
            lhs = Expr {
                loc,
                kind: ExprKind::Assign(
                    Box::new(lhs),
                    Box::new(self.parse_expr_precedence(Precedence::None)?),
                ),
            };
            /*Err(non_place) => {
                lhs = non_place;
                self.diag
                    .add_diagnostic("Invalid assignment target".to_string(), loc);
                break;
            }*/
        }
        Ok(lhs)
    }
    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_single_expr()
    }
    fn parse_single_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_assign()
    }
    fn parse_optional_generics(&mut self) -> Result<Option<Generics>, ParseError> {
        if let Some(Token { loc, .. }) = self.match_token(&TokenKind::LeftBracket) {
            let mut names = Vec::new();
            while let Some(name) = self.match_ident() {
                names.push(name);
                if self.not_matches_token(&TokenKind::Coma) {
                    break;
                }
            }
            let _ = self.expect(&TokenKind::RightBracket);
            Ok(Some(Generics { loc, names }))
        } else {
            Ok(None)
        }
    }
    fn parse_record_field(&mut self) -> Result<RecordField, ParseError> {
        let name = self.expect_ident("record field")?;
        let _ = self.expect(&TokenKind::Colon);
        let ty = self.parse_type()?;
        Ok(RecordField { name, ty })
    }
    fn parse_record_type(&mut self) -> Result<RecordType, ParseError> {
        let _ = self.expect(&TokenKind::LeftBrace);
        let mut fields = Vec::new();
        while self.check_is_not_token(&TokenKind::RightBrace) {
            fields.push(self.parse_record_field()?);
            if self.not_matches_token(&TokenKind::Coma) {
                break;
            }
        }
        let _ = self.expect(&TokenKind::RightBrace);
        Ok(RecordType { fields })
    }
    fn parse_type_function(&mut self) -> Result<FunctionType, ParseError> {
        let _ = self.expect(&TokenKind::Fun);
        let _ = self.expect(&TokenKind::LeftParen);
        let mut params = Vec::new();
        while self.check_is_not_token(&TokenKind::RightParen) {
            params.push(self.parse_type()?);
            if self.not_matches_token(&TokenKind::Coma) {
                break;
            }
        }
        let _ = self.expect(&TokenKind::RightParen);
        let is_resource = self.parse_resource_arrow().unwrap_or(IsResource::Data);

        let return_type = self.parse_type()?;
        Ok(FunctionType {
            resource: is_resource,
            params,
            return_type: Box::new(return_type),
        })
    }
    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let loc = self.current_loc();
        fn type_parse_error(this: &mut Parser, loc: SrcLoc) -> ParseError {
            let msg = if let Some(kind) = this.peek_token().map(|token| &token.kind) {
                format!("Expected a type but got '{kind}'",)
            } else {
                "Expected a type but got eof".to_string()
            };
            this.diag.add_diagnostic(msg, loc);
            ParseError
        }
        let Some(Token { loc: _, kind }) = self.peek_token() else {
            return Err(type_parse_error(self, loc));
        };
        match kind {
            TokenKind::Mut => {
                self.next_token();
                let _ = self.expect(&TokenKind::LeftBracket);
                let region = self.parse_region()?;
                let _ = self.expect(&TokenKind::RightBracket);
                let ty = self.parse_type()?;
                Ok(Type {
                    loc,
                    kind: TypeKind::Mut(region, Box::new(ty)),
                })
            }
            TokenKind::Imm => {
                self.next_token();
                let _ = self.expect(&TokenKind::LeftBracket);
                let region = self.parse_region()?;
                let _ = self.expect(&TokenKind::RightBracket);
                let ty = self.parse_type()?;
                Ok(Type {
                    loc,
                    kind: TypeKind::Imm(region, Box::new(ty)),
                })
            }
            TokenKind::Int => {
                self.next_token();
                Ok(Type {
                    loc,
                    kind: TypeKind::Int,
                })
            }
            TokenKind::Bool => {
                self.next_token();
                Ok(Type {
                    loc,
                    kind: TypeKind::Bool,
                })
            }
            TokenKind::String => {
                self.next_token();
                Ok(Type {
                    loc,
                    kind: TypeKind::String,
                })
            }
            TokenKind::ArrayList => {
                self.next_token();
                let _ = self.expect(&TokenKind::LeftBracket);
                let ty = self.parse_type()?;
                let _ = self.expect(&TokenKind::RightBracket);
                Ok(Type {
                    loc,
                    kind: TypeKind::List(Box::new(ty)),
                })
            }
            TokenKind::LeftParen => {
                self.next_token();
                if self.matches_token(&TokenKind::RightParen) {
                    Ok(Type {
                        loc,
                        kind: TypeKind::Unit,
                    })
                } else {
                    let ty = self.parse_type()?;
                    let _ = self.expect(&TokenKind::RightParen);
                    Ok(ty)
                }
            }
            TokenKind::Option => {
                self.next_token();
                let _ = self.expect(&TokenKind::LeftBracket);
                let ty = self.parse_type()?;
                let _ = self.expect(&TokenKind::RightBracket);
                Ok(Type {
                    loc,
                    kind: TypeKind::Option(Box::new(ty)),
                })
            }
            TokenKind::Ident(_) => {
                let name = self.match_ident().expect("Expected valid ident");
                Ok(Type {
                    loc,
                    kind: TypeKind::Named(name),
                })
            }
            TokenKind::Fun => {
                let function = self.parse_type_function()?;
                Ok(Type {
                    loc,
                    kind: TypeKind::Function(function),
                })
            }
            TokenKind::Box => {
                self.next_token();
                let _ = self.expect(&TokenKind::LeftBracket);
                let ty = self.parse_type()?;
                let _ = self.expect(&TokenKind::RightBracket);
                Ok(Type {
                    loc,
                    kind: TypeKind::Box(Box::new(ty)),
                })
            }
            TokenKind::Char => {
                self.next_token();
                Ok(Type {
                    loc,
                    kind: TypeKind::Char,
                })
            }
            TokenKind::LeftBrace => {
                let record_ty = self.parse_record_type()?;
                Ok(Type {
                    loc,
                    kind: TypeKind::Record(record_ty),
                })
            }
            _ => Err(type_parse_error(self, loc)),
        }
    }
    fn parse_param(&mut self) -> Result<Param, ParseError> {
        let name = self.expect_ident("param name")?;
        let _ = self.expect(&TokenKind::Colon);
        let ty = self.parse_type()?;
        Ok(Param { name, ty })
    }
    fn parse_annotations(&mut self) -> Result<Vec<Annotation>, ParseError> {
        let mut annotations = Vec::new();
        while let Some(token) = self.match_token(&TokenKind::At) {
            let loc = token.loc;
            let name = self.expect_ident("annotation name")?;
            let mut fields = Vec::new();
            if self.matches_token(&TokenKind::LeftParen) {
                while self.not_matches_token(&TokenKind::RightParen) {
                    let (loc, string) = self.match_string_literal().ok_or_else(|| {
                        self.diag
                            .add_diagnostic(format!("Expected a string"), loc.clone());
                        ParseError
                    })?;
                    fields.push(AnnotationField::String(loc, string));
                    if self.not_matches_token(&TokenKind::Coma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RightParen)?;
            }
            annotations.push(Annotation { loc, name, fields });
        }
        Ok(annotations)
    }
    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let annotations = self.parse_annotations()?;
        let loc = self.current_loc();
        let _ = self.expect(&TokenKind::Fun);
        let name = self.expect_ident("function name")?;
        let generics = self.parse_optional_generics()?;
        let _ = self.expect(&TokenKind::LeftParen);
        let mut params = Vec::new();
        while self.check_token_is_ident() {
            let param = self.parse_param()?;
            params.push(param);
            if self.not_matches_token(&TokenKind::Coma) {
                break;
            }
        }
        let _ = self.expect(&TokenKind::RightParen);
        let _ = self.expect(&TokenKind::Arrow);
        let ty = self.parse_type()?;
        let body = if self.matches_token(&TokenKind::Semi) {
            None
        } else {
            let _ = self.expect(&TokenKind::Equal);
            let body = self.parse_expr()?;
            Some(body)
        };
        Ok(Function {
            loc,
            annotations,
            name,
            generics,
            params,
            return_type: ty,
            body,
        })
    }
    pub fn parse_module(mut self) -> Result<Module, ParseError> {
        let mut functions = Vec::new();
        while self.peek_token().is_some() {
            let Ok(function) = self.parse_function() else {
                while self.check_is_not_token(&TokenKind::Fun) {
                    self.next_token();
                }
                continue;
            };
            functions.push(function);
        }
        if self.diag.report_all() {
            return Err(ParseError);
        }
        Ok(Module { functions })
    }
}
