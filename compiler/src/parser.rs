//! Hand-written recursive descent parser for `type` and `schema`.

use crate::ast::*;
use crate::diag::Diag;
use crate::lexer::{Tok, Token};

/// `type Port = u16 where ...;`, from the word `type` on.
pub fn parse_type(tokens: Vec<Token>) -> Result<TypeDecl, Diag> {
    let mut p = Parser { toks: tokens, pos: 0, enum_owner: None, hoisted: Vec::new() };
    let lo = p.bump().span;
    p.type_decl(lo)
}

/// `[open] schema S { ... }`, with the inline enums of its fields.
pub fn parse_schema(tokens: Vec<Token>) -> Result<(SchemaDecl, Vec<EnumDecl>), Diag> {
    let mut p = Parser { toks: tokens, pos: 0, enum_owner: None, hoisted: Vec::new() };
    let lo = p.span();
    let open = p.eat_kw("open");
    p.bump();
    let schema = p.schema(lo, open)?;
    Ok((schema, p.hoisted))
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// Name for an inline `enum { ... }` in the field being parsed.
    enum_owner: Option<String>,
    /// Inline enums of the current item, to be declared before it.
    hoisted: Vec<EnumDecl>,
}

type PResult<T> = Result<T, Diag>;

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Int(s) | Tok::Float(s) => format!("数値 `{s}`"),
        Tok::Str { .. } => "文字列".into(),
        Tok::Punct(p) => format!("`{p}`"),
        Tok::Eof => "ファイルの終わり".into(),
    }
}

impl Parser {
    fn tok(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn nth(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn at_eof(&self) -> bool {
        matches!(self.tok(), Tok::Eof)
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if !self.at_eof() {
            self.pos += 1;
        }
        t
    }

    fn is(&self, p: &str) -> bool {
        matches!(self.tok(), Tok::Punct(q) if *q == p)
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.tok(), Tok::Ident(s) if s == kw)
    }

    fn eat(&mut self, p: &str) -> bool {
        let ok = self.is(p);
        if ok {
            self.bump();
        }
        ok
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        let ok = self.is_kw(kw);
        if ok {
            self.bump();
        }
        ok
    }

    fn unexpected(&self, wanted: &str) -> Diag {
        Diag::new(self.span(), format!("{wanted}が必要ですが、{}があります", describe(self.tok())))
    }

    fn expect(&mut self, p: &str) -> PResult<Span> {
        if self.is(p) {
            Ok(self.bump().span)
        } else if p == ";" && self.pos > 0 {
            // Point just after the previous token, where the `;` belongs.
            let at = self.prev_span().hi;
            Err(Diag::new(Span { lo: at, hi: at }, "`;` が必要です"))
        } else {
            Err(self.unexpected(&format!("`{p}` ")))
        }
    }

    fn ident(&mut self) -> PResult<Name> {
        match self.tok().clone() {
            Tok::Ident(text) => {
                let span = self.bump().span;
                Ok(Name { text, span })
            }
            _ => Err(self.unexpected("名前")),
        }
    }

    fn string(&mut self) -> PResult<String> {
        match self.tok().clone() {
            Tok::Str { value, .. } => {
                self.bump();
                Ok(value)
            }
            _ => Err(self.unexpected("文字列")),
        }
    }

    // -------------------------------------------------------- declarations

    fn type_decl(&mut self, lo: Span) -> PResult<TypeDecl> {
        let name = self.ident()?;
        self.expect("=")?;
        let base = self.ty()?;
        let (mut cond, mut matches) = (None, None);
        loop {
            if self.eat_kw("where") {
                cond = Some(self.expr()?);
            } else if self.eat_kw("matches") {
                matches = Some(self.ident()?);
            } else {
                break;
            }
        }
        let end = self.expect(";")?;
        Ok(TypeDecl { name, base, cond, matches, span: lo.to(end) })
    }

    /// `{ a, b }`
    fn enum_body(&mut self) -> PResult<(Vec<Name>, Span)> {
        self.expect("{")?;
        let mut variants = Vec::new();
        while !self.is("}") {
            variants.push(self.ident()?);
            if !self.eat(",") {
                break;
            }
        }
        let end = self.expect("}")?;
        Ok((variants, end))
    }

    fn schema(&mut self, lo: Span, open: bool) -> PResult<SchemaDecl> {
        let name = self.ident()?;
        self.expect("{")?;
        let (mut fields, mut invariants) = (Vec::new(), Vec::new());
        while !self.is("}") {
            if self.is_kw("invariant") && matches!(self.nth(1), Tok::Punct("(")) {
                let lo = self.bump().span;
                invariants.push(self.clause(lo)?);
                self.expect(";")?;
            } else {
                fields.push(self.field(&name.text)?);
            }
        }
        let end = self.expect("}")?;
        Ok(SchemaDecl { open, name, fields, invariants, span: lo.to(end) })
    }

    fn field(&mut self, schema: &str) -> PResult<Field> {
        let name = self.ident()?;
        self.expect(":")?;
        self.enum_owner = Some(format!("{schema}_{}", name.text));
        let ty = self.ty()?;
        self.enum_owner = None;
        let (mut cond, mut matches, mut transform, mut default) = (None, None, None, None);
        loop {
            if self.eat_kw("where") {
                cond = Some(self.expr()?);
            } else if self.eat_kw("matches") {
                matches = Some(self.ident()?);
            } else if self.eat_kw("transform") {
                transform = Some(self.ident()?);
            } else if self.eat("=") {
                default = Some(self.expr()?);
            } else {
                break;
            }
        }
        let end = self.expect(";")?;
        Ok(Field { span: name.span.to(end), name, ty, cond, matches, transform, default })
    }

    /// `( expr [, "label"] )`, after the keyword.
    fn clause(&mut self, lo: Span) -> PResult<Clause> {
        self.expect("(")?;
        let cond = self.expr()?;
        let label = if self.eat(",") { Some(self.string()?) } else { None };
        let end = self.expect(")")?;
        Ok(Clause { cond, label, span: lo.to(end) })
    }

    // ---------------------------------------------------------------- types

    fn ty(&mut self) -> PResult<Type> {
        let mut name = self.ident()?;
        while self.is("::") {
            self.bump();
            let next = self.ident()?;
            name = Name { text: format!("{}::{}", name.text, next.text), span: name.span.to(next.span) };
        }
        let mut span = name.span;
        let kind = match name.text.as_str() {
            "enum" if self.is("{") => {
                let Some(owner) = self.enum_owner.take() else {
                    return Err(Diag::new(name.span, "`enum { ... }` はスキーマのフィールドにだけ書けます")
                        .note("それ以外の場所では `enum Name { ... }` を宣言して使ってください"));
                };
                let (variants, end) = self.enum_body()?;
                span = span.to(end);
                self.hoisted.push(EnumDecl { name: Name { text: owner.clone(), span: name.span }, variants, span });
                TypeKind::Named(owner)
            }
            "vec" | "std::vector" | "Raw" | "cc::Raw" | "std::optional" => {
                self.expect("<")?;
                let inner = Box::new(self.ty()?);
                span = span.to(self.expect(">")?);
                match name.text.as_str() {
                    "vec" | "std::vector" => TypeKind::Vec(inner),
                    "std::optional" => TypeKind::Optional(inner),
                    _ => TypeKind::Raw(inner),
                }
            }
            "Json" | "cc::Json" => TypeKind::Json,
            "std::string" => TypeKind::Prim(Prim::String),
            n => match Prim::from_name(n) {
                Some(p) => TypeKind::Prim(p),
                None => TypeKind::Named(n.to_string()),
            },
        };
        let mut ty = Type { kind, span };
        while self.is("?") {
            let end = self.bump().span;
            ty = Type { span: ty.span.to(end), kind: TypeKind::Optional(Box::new(ty)) };
        }
        Ok(ty)
    }

    // ---------------------------------------------------------- expressions

    pub fn expr(&mut self) -> PResult<Expr> {
        self.binary(0)
    }

    fn binary(&mut self, level: usize) -> PResult<Expr> {
        const LEVELS: &[&[(&str, BinOp)]] = &[
            &[("||", BinOp::Or)],
            &[("&&", BinOp::And)],
            &[], // comparisons
            &[("+", BinOp::Add), ("-", BinOp::Sub)],
            &[("*", BinOp::Mul), ("/", BinOp::Div), ("%", BinOp::Rem)],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        if level == 2 {
            return self.comparison();
        }
        let mut lhs = self.binary(level + 1)?;
        'outer: loop {
            for (p, op) in LEVELS[level] {
                if self.eat(p) {
                    let rhs = self.binary(level + 1)?;
                    lhs =
                        Expr { span: lhs.span.to(rhs.span), kind: ExprKind::Binary(*op, Box::new(lhs), Box::new(rhs)) };
                    continue 'outer;
                }
            }
            return Ok(lhs);
        }
    }

    fn comparison(&mut self) -> PResult<Expr> {
        let first = self.binary(3)?;
        let mut rest = Vec::new();
        loop {
            let op = match self.tok() {
                Tok::Punct("==") => CmpOp::Eq,
                Tok::Punct("!=") => CmpOp::Ne,
                Tok::Punct("<") => CmpOp::Lt,
                Tok::Punct("<=") => CmpOp::Le,
                Tok::Punct(">") => CmpOp::Gt,
                Tok::Punct(">=") => CmpOp::Ge,
                _ => break,
            };
            self.bump();
            rest.push((op, self.binary(3)?));
        }
        if rest.is_empty() {
            return Ok(first);
        }
        let span = first.span.to(rest.last().unwrap().1.span);
        Ok(Expr { kind: ExprKind::Compare(Box::new(first), rest), span })
    }

    fn unary(&mut self) -> PResult<Expr> {
        let lo = self.span();
        let op = if self.eat("-") {
            UnOp::Neg
        } else if self.eat("!") {
            UnOp::Not
        } else {
            return self.postfix();
        };
        let e = self.unary()?;
        Ok(Expr { span: lo.to(e.span), kind: ExprKind::Unary(op, Box::new(e)) })
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.eat("(") {
                let args = self.args(")")?;
                let span = e.span.to(self.prev_span());
                e = Expr { kind: ExprKind::Call(Box::new(e), args), span };
            } else if self.eat(".") {
                let f = self.ident()?;
                e = Expr { span: e.span.to(f.span), kind: ExprKind::Field(Box::new(e), f.text) };
            } else if self.eat("[") {
                let i = self.expr()?;
                let end = self.expect("]")?;
                e = Expr { span: e.span.to(end), kind: ExprKind::Index(Box::new(e), Box::new(i)) };
            } else {
                return Ok(e);
            }
        }
    }

    fn args(&mut self, close: &str) -> PResult<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.is(close) {
            args.push(self.expr()?);
            if !self.eat(",") {
                break;
            }
        }
        self.expect(close)?;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.bump();
        let span = t.span;
        let kind = match t.tok {
            Tok::Int(s) => ExprKind::Int(s),
            Tok::Float(s) => ExprKind::Float(s),
            Tok::Str { lit, .. } => ExprKind::Str { lit },
            Tok::Punct("(") => {
                let mut e = self.expr()?;
                e.span = span.to(self.expect(")")?);
                return Ok(e);
            }
            Tok::Punct("[") => {
                let items = self.args("]")?;
                return Ok(Expr { kind: ExprKind::List(items), span: span.to(self.prev_span()) });
            }
            Tok::Ident(s) => match s.as_str() {
                "true" => ExprKind::Bool(true),
                "false" => ExprKind::Bool(false),
                "none" => ExprKind::None,
                "self" => ExprKind::SelfValue,
                _ if self.is("::") => {
                    let mut prefix = s;
                    let mut end;
                    loop {
                        self.bump();
                        end = self.ident()?;
                        if !self.is("::") {
                            break;
                        }
                        prefix = format!("{prefix}::{}", end.text);
                    }
                    return Ok(Expr { kind: ExprKind::Path(prefix, end.text), span: span.to(end.span) });
                }
                _ => ExprKind::Ident(s),
            },
            other => {
                if !matches!(other, Tok::Eof) {
                    self.pos -= 1;
                }
                return Err(Diag::new(span, format!("式が必要ですが、{}があります", describe(&other))));
            }
        };
        Ok(Expr { kind, span })
    }
}
