//! Checks on `type` and `schema` that need no C++ knowledge: what a `where` may
//! contain, duplicate names, and which enum a bare value belongs to. Everything
//! else about types is left to the C++ compiler, and effects to `effects.rs`.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diag::Diag;

/// Every effect name the language knows.
pub const EFFECTS: &[&str] = &["io", "alloc", "throw", "async", "unsafe", "nondet", "log"];

/// Names that `where`, `invariant`, `matches` and `transform` take from `cc::`.
pub fn is_builtin(name: &str) -> bool {
    matches!(name, "len" | "trim" | "email_format")
}

pub enum TypeInfo<'a> {
    Refined,
    Schema(&'a SchemaDecl),
    Enum(&'a EnumDecl),
}

/// Name tables shared by the checker and the emitter.
pub struct Ctx<'a> {
    pub types: HashMap<&'a str, TypeInfo<'a>>,
    /// Bare enum values (`role != admin`) by span start, with their C++ spelling.
    pub variants: HashMap<usize, String>,
}

impl<'a> Ctx<'a> {
    fn named(&self, t: &Type) -> Option<&TypeInfo<'a>> {
        match &t.kind {
            TypeKind::Named(n) => self.types.get(n.as_str()),
            _ => None,
        }
    }

    fn schema(&self, t: &Type) -> Option<&'a SchemaDecl> {
        match self.named(t) {
            Some(TypeInfo::Schema(d)) => Some(d),
            _ => None,
        }
    }

    fn enum_of(&self, t: &Type) -> Option<&'a EnumDecl> {
        match self.named(t) {
            Some(TypeInfo::Enum(d)) => Some(d),
            _ => None,
        }
    }
}

pub fn check<'a>(
    types: &'a [TypeDecl],
    schemas: &'a [SchemaDecl],
    enums: &'a [EnumDecl],
    src: &str,
) -> (Ctx<'a>, Vec<Diag>) {
    let mut c = Checker {
        ctx: Ctx { types: HashMap::new(), variants: HashMap::new() },
        src,
        diags: Vec::new(),
        fields: HashMap::new(),
    };
    let mut seen: HashSet<&str> = HashSet::new();
    let names = types
        .iter()
        .map(|d| (&d.name, TypeInfo::Refined))
        .chain(schemas.iter().map(|d| (&d.name, TypeInfo::Schema(d))))
        .chain(enums.iter().map(|d| (&d.name, TypeInfo::Enum(d))));
    for (name, info) in names {
        if !seen.insert(&name.text) {
            c.diags.push(Diag::new(name.span, format!("`{}` はすでに宣言されています", name.text)));
        } else {
            c.ctx.types.insert(&name.text, info);
        }
    }
    enums.iter().for_each(|d| c.enum_decl(d));
    types.iter().for_each(|d| c.type_decl(d));
    schemas.iter().for_each(|d| c.schema(d));
    c.diags.sort_by_key(|d| d.span.lo);
    (c.ctx, c.diags)
}

struct Checker<'a, 's> {
    ctx: Ctx<'a>,
    src: &'s str,
    diags: Vec<Diag>,
    /// Fields of the schema being checked, which is what names mean in an `invariant`.
    fields: HashMap<&'a str, &'a Type>,
}

impl<'a> Checker<'a, '_> {
    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diag::new(span, msg));
    }

    fn text(&self, span: Span) -> &str {
        &self.src[span.lo..span.hi]
    }

    fn value_type(&mut self, t: &Type) {
        match &t.kind {
            TypeKind::Raw(_) => {
                self.err(t.span, "Raw<T> はスキーマのフィールドには使えません（関数の引数で受け取ってください）")
            }
            TypeKind::Optional(i) | TypeKind::Vec(i) => self.value_type(i),
            _ => {}
        }
    }

    /// `where` is limited to what an SMT solver can take later on.
    fn where_expr(&mut self, e: &Expr) {
        let ok = match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Str { .. } | ExprKind::Bool(_) | ExprKind::SelfValue => {
                true
            }
            ExprKind::Unary(_, a) => {
                self.where_expr(a);
                true
            }
            ExprKind::Binary(_, a, b) => {
                self.where_expr(a);
                self.where_expr(b);
                true
            }
            ExprKind::Compare(first, rest) => {
                self.where_expr(first);
                rest.iter().for_each(|(_, r)| self.where_expr(r));
                true
            }
            ExprKind::Call(callee, args)
                if args.len() == 1 && matches!(&callee.kind, ExprKind::Ident(n) if n == "len") =>
            {
                self.where_expr(&args[0]);
                true
            }
            _ => false,
        };
        if !ok {
            self.diags.push(
                Diag::new(e.span, format!("where には `{}` を書けません", self.text(e.span)))
                    .note("使えるのはリテラル・self・len()・算術・比較・論理だけです。それ以外の条件は matches か invariant に書いてください"),
            );
        }
    }

    fn enum_decl(&mut self, d: &EnumDecl) {
        if d.variants.is_empty() {
            self.err(d.span, format!("enum `{}` に値がありません", d.name.text));
        }
        let mut seen = HashSet::new();
        for v in &d.variants {
            if !seen.insert(v.text.as_str()) {
                self.err(v.span, format!("値 `{}` が重複しています", v.text));
            }
        }
    }

    /// `e` is used as a value of type `t`: a bare name means a value of that enum.
    fn variant(&mut self, e: &Expr, t: &Type) {
        let inner = match &t.kind {
            TypeKind::Optional(i) => i.as_ref(),
            _ => t,
        };
        let (Some(d), ExprKind::Ident(n)) = (self.ctx.enum_of(inner), &e.kind) else { return };
        if self.fields.contains_key(n.as_str()) {
            return;
        }
        if d.variants.iter().any(|v| v.text == *n) {
            let cpp = format!("{}::{}", crate::emit::mangle(&d.name.text), crate::emit::mangle(n));
            self.ctx.variants.insert(e.span.lo, cpp);
        } else {
            let names = d.variants.iter().map(|v| v.text.as_str()).collect::<Vec<_>>().join(", ");
            self.diags.push(
                Diag::new(e.span, format!("`{n}` は {} の値ではありません", d.name.text))
                    .note(format!("使える値: {names}")),
            );
        }
    }

    fn type_decl(&mut self, d: &'a TypeDecl) {
        match &d.base.kind {
            TypeKind::Prim(_) | TypeKind::Json | TypeKind::Vec(_) => self.value_type(&d.base),
            _ => self.err(d.base.span, "type の元にできるのは基本型・Json・vec<T> だけです"),
        }
        if d.cond.is_none() && d.matches.is_none() {
            self.err(d.span, format!("型 `{}` に条件がありません（where か matches が必要です）", d.name.text));
        }
        if let Some(c) = &d.cond {
            self.where_expr(c);
        }
    }

    fn schema(&mut self, d: &'a SchemaDecl) {
        let mut seen = HashSet::new();
        self.fields.clear();
        for f in &d.fields {
            if !seen.insert(f.name.text.as_str()) {
                self.err(f.name.span, format!("フィールド `{}` が重複しています", f.name.text));
            }
            self.value_type(&f.ty);
            let inner = match &f.ty.kind {
                TypeKind::Optional(i) => i.as_ref(),
                _ => &f.ty,
            };
            if let Some(c) = &f.cond {
                self.where_expr(c);
            }
            if f.transform.is_some() && self.ctx.schema(inner).is_some() {
                self.err(f.span, "スキーマ型のフィールドには transform を使えません");
            }
            if let Some(def) = &f.default {
                self.expr(def, false);
                self.variant(def, inner);
            }
        }
        for f in &d.fields {
            self.fields.insert(&f.name.text, &f.ty);
        }
        for inv in &d.invariants {
            self.expr(&inv.cond, false);
        }
    }

    fn expr(&mut self, e: &Expr, self_ok: bool) {
        match &e.kind {
            ExprKind::Path(t, m) => {
                if let Some(TypeInfo::Enum(d)) = self.ctx.types.get(t.as_str()) {
                    if !d.variants.iter().any(|v| v.text == *m) {
                        self.err(e.span, format!("`{m}` は {t} の値ではありません"));
                    }
                }
            }
            ExprKind::SelfValue if !self_ok => self.err(e.span, "self は where 条件の中でだけ使えます"),
            ExprKind::List(items) => items.iter().for_each(|i| self.expr(i, self_ok)),
            ExprKind::Field(obj, _) => self.expr(obj, self_ok),
            ExprKind::Index(a, i) | ExprKind::Binary(_, a, i) => {
                self.expr(a, self_ok);
                self.expr(i, self_ok);
            }
            ExprKind::Unary(_, a) => self.expr(a, self_ok),
            ExprKind::Compare(first, rest) => {
                self.expr(first, self_ok);
                rest.iter().for_each(|(_, r)| self.expr(r, self_ok));
                let mut lhs = first.as_ref();
                for (_, r) in rest {
                    self.link_variants(lhs, r);
                    lhs = r;
                }
            }
            ExprKind::Call(callee, args) => {
                if !matches!(callee.kind, ExprKind::Ident(_)) {
                    self.expr(callee, self_ok);
                }
                args.iter().for_each(|a| self.expr(a, self_ok));
            }
            _ => {}
        }
    }

    fn link_variants(&mut self, a: &Expr, b: &Expr) {
        if let Some(t) = self.type_of(a) {
            self.variant(b, &t);
        }
        if let Some(t) = self.type_of(b) {
            self.variant(a, &t);
        }
    }

    /// The type of a field or `Enum::value`, which is all that bare values need.
    fn type_of(&self, e: &Expr) -> Option<Type> {
        match &e.kind {
            ExprKind::Ident(n) => self.fields.get(n.as_str()).map(|t| (*t).clone()),
            ExprKind::Path(t, _) if matches!(self.ctx.types.get(t.as_str()), Some(TypeInfo::Enum(_))) => {
                Some(Type { kind: TypeKind::Named(t.clone()), span: e.span })
            }
            ExprKind::Field(obj, f) => {
                let s = self.ctx.schema(&self.type_of(obj)?)?;
                s.fields.iter().find(|x| &x.name.text == f).map(|x| x.ty.clone())
            }
            _ => None,
        }
    }
}
