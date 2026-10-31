//! Hand-written recursive descent parser.

use crate::ast::*;
use crate::diag::Diag;
use crate::lexer::{Tok, Token};

pub fn parse(tokens: Vec<Token>) -> Result<Program, Diag> {
    let mut p = Parser { toks: tokens, pos: 0, enum_owner: None, hoisted: Vec::new(), in_test: false };
    let mut items = Vec::new();
    while !p.at_eof() {
        let item = p.item()?;
        items.extend(p.hoisted.drain(..).map(Item::Enum));
        items.push(item);
    }
    Ok(Program { items })
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// Name for an inline `enum { ... }` in the field being parsed.
    enum_owner: Option<String>,
    /// Inline enums of the current item, to be declared before it.
    hoisted: Vec<EnumDecl>,
    in_test: bool,
}

type PResult<T> = Result<T, Diag>;

fn describe(t: &Tok) -> String {
    match t {
        Tok::Ident(s) => format!("`{s}`"),
        Tok::Int(s) | Tok::Float(s) => format!("数値 `{s}`"),
        Tok::Str { .. } => "文字列".into(),
        Tok::Cpp(_) => "cpp ブロック".into(),
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

    // ---------------------------------------------------------------- items

    fn item(&mut self) -> PResult<Item> {
        let lo = self.span();
        if let Tok::Cpp(code) = self.tok().clone() {
            self.bump();
            return Ok(Item::Cpp { code, span: lo });
        }
        if self.eat_kw("type") {
            return self.type_decl(lo).map(Item::Type);
        }
        if self.is_kw("open") && matches!(self.nth(1), Tok::Ident(s) if s == "schema") {
            self.bump();
            self.bump();
            return self.schema(lo, true).map(Item::Schema);
        }
        if self.eat_kw("schema") {
            return self.schema(lo, false).map(Item::Schema);
        }
        if self.eat_kw("enum") {
            let name = self.ident()?;
            let (variants, end) = self.enum_body()?;
            return Ok(Item::Enum(EnumDecl { name, variants, span: lo.to(end) }));
        }
        if self.is_kw("test") && matches!(self.nth(1), Tok::Str { .. }) {
            self.bump();
            let label = self.string()?;
            self.in_test = true;
            let body = self.block()?;
            self.in_test = false;
            let span = lo.to(self.prev_span());
            let func = FnDecl {
                name: Name { text: "test".into(), span: lo },
                is_contract: false,
                params: Vec::new(),
                ret: None,
                effects: None,
                pres: Vec::new(),
                posts: Vec::new(),
                body: Some(body),
                span,
            };
            return Ok(Item::Test(TestDecl { label, func }));
        }
        let is_extern = self.eat_kw("extern");
        let is_contract = self.eat_kw("contract");
        if self.eat_kw("fn") {
            return self.fn_decl(lo, is_contract, is_extern).map(Item::Fn);
        }
        Err(self.unexpected("宣言（type / schema / enum / fn / contract fn / test / cpp）"))
    }

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

    fn fn_decl(&mut self, lo: Span, is_contract: bool, is_extern: bool) -> PResult<FnDecl> {
        let name = self.ident()?;
        self.expect("(")?;
        let mut params = Vec::new();
        while !self.is(")") {
            let name = self.ident()?;
            if !self.eat(":") {
                return Err(Diag::new(name.span, "引数は `名前: 型` の順で書きます")
                    .note("例: `fn withdraw(a: Account&, amt: i64)`"));
            }
            let ty = self.ty()?;
            let by_ref = self.eat("&");
            params.push(Param { name, ty, by_ref });
            if !self.eat(",") {
                break;
            }
        }
        self.expect(")")?;
        let ret = if self.eat("->") { Some(self.ty()?) } else { None };
        let (mut effects, mut pres, mut posts) = (None, Vec::new(), Vec::new());
        loop {
            let clause_lo = self.span();
            if self.eat_kw("effects") {
                self.expect("(")?;
                let mut list = Vec::new();
                while !self.is(")") {
                    list.push(self.ident()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(")")?;
                effects = Some(list);
            } else if self.eat_kw("pre") {
                pres.push(self.clause(clause_lo)?);
            } else if self.eat_kw("post") {
                self.expect("(")?;
                let binder = if matches!(self.tok(), Tok::Ident(_)) && matches!(self.nth(1), Tok::Punct(":")) {
                    let b = self.ident()?;
                    self.bump();
                    Some(b)
                } else {
                    None
                };
                let cond = self.expr()?;
                let label = if self.eat(",") { Some(self.string()?) } else { None };
                let end = self.expect(")")?;
                posts.push(PostClause { binder, cond, label, span: clause_lo.to(end) });
            } else {
                break;
            }
        }
        let body = if self.eat(";") {
            None
        } else if is_extern {
            return Err(self.unexpected("extern fn の後には `;` "));
        } else {
            Some(self.block()?)
        };
        Ok(FnDecl { span: lo.to(self.prev_span()), name, is_contract, params, ret, effects, pres, posts, body })
    }

    // ---------------------------------------------------------------- types

    fn ty(&mut self) -> PResult<Type> {
        let name = self.ident()?;
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
            "vec" | "Raw" => {
                self.expect("<")?;
                let inner = Box::new(self.ty()?);
                span = span.to(self.expect(">")?);
                if name.text == "vec" {
                    TypeKind::Vec(inner)
                } else {
                    TypeKind::Raw(inner)
                }
            }
            "Json" => TypeKind::Json,
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

    // ----------------------------------------------------------- statements

    fn block(&mut self) -> PResult<Block> {
        let lo = self.expect("{")?;
        let mut stmts = Vec::new();
        while !self.is("}") {
            if self.at_eof() {
                return Err(Diag::new(lo, "`{` が閉じていません"));
            }
            stmts.push(self.stmt()?);
        }
        let hi = self.expect("}")?;
        Ok(Block { stmts, span: lo.to(hi) })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let lo = self.span();
        let kind = if let Tok::Cpp(code) = self.tok().clone() {
            self.bump();
            StmtKind::Cpp(code)
        } else if self.is("{") {
            StmtKind::Block(self.block()?)
        } else if self.eat_kw("let") {
            let name = self.ident()?.text;
            let ty = if self.eat(":") { Some(self.ty()?) } else { None };
            self.expect("=")?;
            let init = self.expr()?;
            self.expect(";")?;
            StmtKind::Let { name, ty, init }
        } else if self.eat_kw("return") {
            let value = if self.is(";") { None } else { Some(self.expr()?) };
            self.expect(";")?;
            StmtKind::Return(value)
        } else if self.eat_kw("if") {
            return self.if_stmt(lo);
        } else if self.eat_kw("for") {
            let var = self.ident()?.text;
            if !self.eat_kw("in") {
                return Err(self.unexpected("`in` "));
            }
            let first = self.expr()?;
            let iter = if self.eat("..") { ForIter::Range(first, self.expr()?) } else { ForIter::Each(first) };
            StmtKind::For { var, iter, body: self.block()? }
        } else if self.eat_kw("while") {
            let cond = self.expr()?;
            StmtKind::While { cond, body: self.block()? }
        } else if self.eat_kw("match") {
            let subject = self.expr()?;
            self.expect("{")?;
            let mut arms = Vec::new();
            while !self.is("}") {
                arms.push(self.match_arm()?);
            }
            self.expect("}")?;
            StmtKind::Match { subject, arms }
        } else if self.is_kw("try") && matches!(self.nth(1), Tok::Punct("{")) {
            self.bump();
            let body = self.block()?;
            if !self.eat_kw("catch") {
                return Err(self.unexpected("`catch` "));
            }
            let var = self.ident()?;
            StmtKind::Try { body, var, handler: self.block()? }
        } else if self.is_kw("with") && matches!(self.nth(2), Tok::Punct("=")) {
            if !self.in_test {
                return Err(Diag::new(lo, "with は test の中でだけ使えます"));
            }
            self.bump();
            let effect = self.ident()?;
            self.expect("=")?;
            let handler = self.ident()?;
            self.expect("{")?;
            let mut entries = Vec::new();
            while !self.is("}") {
                self.expect("{")?;
                let key = self.expr()?;
                self.expect(",")?;
                let value = self.expr()?;
                self.expect("}")?;
                entries.push((key, value));
                if !self.eat(",") {
                    break;
                }
            }
            self.expect("}")?;
            StmtKind::With { effect, handler, entries, body: self.block()? }
        } else if self.in_test && self.is_kw("expect") && matches!(self.nth(1), Tok::Punct("(")) {
            self.bump();
            self.bump();
            let cond = self.expr()?;
            self.expect(")")?;
            self.expect(";")?;
            StmtKind::Expect(cond)
        } else {
            let target = self.expr()?;
            let op = match self.tok() {
                Tok::Punct("=") => Some(AssignOp::Set),
                Tok::Punct("+=") => Some(AssignOp::Add),
                Tok::Punct("-=") => Some(AssignOp::Sub),
                Tok::Punct("*=") => Some(AssignOp::Mul),
                Tok::Punct("/=") => Some(AssignOp::Div),
                Tok::Punct("%=") => Some(AssignOp::Rem),
                _ => None,
            };
            let kind = match op {
                Some(op) => {
                    self.bump();
                    StmtKind::Assign { target, op, value: self.expr()? }
                }
                None => StmtKind::Expr(target),
            };
            self.expect(";")?;
            kind
        };
        Ok(Stmt { kind, span: lo.to(self.prev_span()) })
    }

    /// `ok(v) { ... }`, `err(e) { ... }`, `some(x) { ... }`, `none { ... }`
    fn match_arm(&mut self) -> PResult<MatchArm> {
        let word = self.ident()?;
        let kind = match word.text.as_str() {
            "ok" => ArmKind::Ok,
            "err" => ArmKind::Err,
            "some" => ArmKind::Some,
            "none" => ArmKind::None,
            other => {
                return Err(Diag::new(word.span, format!("`{other}` は match の分岐に使えません"))
                    .note("使えるのは ok(v) / err(e) / some(x) / none です"))
            }
        };
        let binder = if kind == ArmKind::None {
            None
        } else {
            self.expect("(")?;
            let b = self.ident()?;
            self.expect(")")?;
            Some(b)
        };
        let body = self.block()?;
        Ok(MatchArm { kind, binder, span: word.span.to(body.span), body })
    }

    fn if_stmt(&mut self, lo: Span) -> PResult<Stmt> {
        let cond = self.expr()?;
        let then = self.block()?;
        let els = if self.eat_kw("else") {
            if self.is_kw("if") {
                let if_lo = self.bump().span;
                let nested = self.if_stmt(if_lo)?;
                Some(Block { span: nested.span, stmts: vec![nested] })
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };
        Ok(Stmt { kind: StmtKind::If { cond, then, els }, span: lo.to(self.prev_span()) })
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
                if matches!(&e.kind, ExprKind::Ident(n) if n == "old") {
                    if args.len() != 1 {
                        return Err(Diag::new(span, "old() は引数を1つ取ります"));
                    }
                    e = Expr { kind: ExprKind::Old(Box::new(args.into_iter().next().unwrap())), span };
                } else {
                    e = Expr { kind: ExprKind::Call(Box::new(e), args), span };
                }
            } else if self.eat(".") {
                let f = self.ident()?;
                e = Expr { span: e.span.to(f.span), kind: ExprKind::Field(Box::new(e), f.text) };
            } else if self.eat("[") {
                let i = self.expr()?;
                let end = self.expect("]")?;
                e = Expr { span: e.span.to(end), kind: ExprKind::Index(Box::new(e), Box::new(i)) };
            } else if self.is("?") {
                let end = self.bump().span;
                e = Expr { span: e.span.to(end), kind: ExprKind::Try(Box::new(e)) };
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
                    self.bump();
                    let m = self.ident()?;
                    return Ok(Expr { kind: ExprKind::Path(s, m.text), span: span.to(m.span) });
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
