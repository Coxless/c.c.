//! Semantic checks: name tables, effects, where `self` / `old` / `?` may
//! appear, and the static parts of value contracts (refined literals and
//! `Raw<T>` vs validated types).

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::ast::*;
use crate::diag::{Diag, Source};

/// Every effect name the language knows, in display order.
pub const EFFECTS: &[&str] = &["io", "alloc", "throw", "async", "unsafe", "nondet", "log"];

pub struct Builtin {
    pub effects: &'static [&'static str],
    pub ret: Option<TypeKind>,
}

pub fn builtin(name: &str) -> Option<Builtin> {
    let (effects, ret): (&[&str], _) = match name {
        "len" => (&[], Some(TypeKind::Prim(Prim::I64))),
        "trim" => (&[], Some(TypeKind::Prim(Prim::String))),
        "email_format" => (&[], Some(TypeKind::Prim(Prim::Bool))),
        "parse_json" => (&[], Some(TypeKind::Raw(Box::new(Type { kind: TypeKind::Json, span: Span::default() })))),
        "read_file" => (&["io", "throw"], Some(TypeKind::Prim(Prim::String))),
        "print" | "eprint" => (&["io"], None),
        "log" => (&["log"], None),
        _ => return None,
    };
    Some(Builtin { effects, ret })
}

pub enum TypeInfo<'a> {
    Refined(&'a TypeDecl),
    Schema(&'a SchemaDecl),
    Enum(&'a EnumDecl),
}

/// Name tables shared by the checker and the emitter.
pub struct Ctx<'a> {
    pub types: HashMap<&'a str, TypeInfo<'a>>,
    pub fns: HashMap<&'a str, &'a FnDecl>,
    /// Bare enum variants (`role != admin`) by span start, with their C++ spelling.
    pub variants: HashMap<usize, String>,
}

impl<'a> Ctx<'a> {
    pub fn refined(&self, t: &Type) -> Option<&'a TypeDecl> {
        match &t.kind {
            TypeKind::Named(n) => match self.types.get(n.as_str()) {
                Some(TypeInfo::Refined(d)) => Some(d),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn schema(&self, t: &Type) -> Option<&'a SchemaDecl> {
        match &t.kind {
            TypeKind::Named(n) => match self.types.get(n.as_str()) {
                Some(TypeInfo::Schema(d)) => Some(d),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn enum_of(&self, t: &Type) -> Option<&'a EnumDecl> {
        match &t.kind {
            TypeKind::Named(n) => match self.types.get(n.as_str()) {
                Some(TypeInfo::Enum(d)) => Some(d),
                _ => None,
            },
            _ => None,
        }
    }

    /// A refined type whose literals can be checked at compile time (`T::of<V>()`).
    pub fn literal_refined(&self, t: &Type) -> Option<&'a TypeDecl> {
        self.refined(t).filter(|d| matches!(d.base.kind, TypeKind::Prim(p) if p.is_integral()))
    }
}

pub fn type_str(t: &Type) -> String {
    match &t.kind {
        TypeKind::Prim(p) => format!("{p:?}").to_lowercase(),
        TypeKind::Json => "Json".into(),
        TypeKind::Named(n) => n.clone(),
        TypeKind::Vec(t) => format!("vec<{}>", type_str(t)),
        TypeKind::Raw(t) => format!("Raw<{}>", type_str(t)),
        TypeKind::Optional(t) => format!("{}?", type_str(t)),
        TypeKind::Result(t, e) => format!("{} または {}", type_str(t), type_str(e)),
    }
}

pub fn effects_str<'s>(effects: impl IntoIterator<Item = &'s str>) -> String {
    let set: BTreeSet<usize> =
        effects.into_iter().map(|e| EFFECTS.iter().position(|x| *x == e).unwrap_or(usize::MAX)).collect();
    set.into_iter().map(|i| EFFECTS[i]).collect::<Vec<_>>().join(", ")
}

/// Integer, negative integer, or bool literal: checkable by `T::of<V>()`.
pub fn integral_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) => true,
        ExprKind::Unary(UnOp::Neg, inner) => matches!(inner.kind, ExprKind::Int(_)),
        _ => false,
    }
}

fn is_literal(e: &Expr) -> bool {
    integral_literal(e)
        || matches!(e.kind, ExprKind::Float(_) | ExprKind::Str { .. })
        || matches!(&e.kind, ExprKind::Unary(UnOp::Neg, i) if matches!(i.kind, ExprKind::Float(_)))
}

/// A literal default of a refined field. Integers are checked at compile time, the
/// others when `parse` fills in the default.
fn literal_fits(e: &Expr, base: &Type) -> bool {
    let TypeKind::Prim(p) = base.kind else { return false };
    match &e.kind {
        ExprKind::Str { .. } => p == Prim::String,
        ExprKind::Float(_) => p == Prim::F64,
        _ => integral_literal(e) && p.is_integral(),
    }
}

/// Something that needs an effect: a call, `?`, or a cpp block.
struct Need {
    span: Span,
    what: String,
    effects: Vec<&'static str>,
    note: Option<String>,
}

#[derive(Clone, Copy)]
struct Allow {
    self_: bool,
    old: bool,
}

const NONE: Allow = Allow { self_: false, old: false };

pub fn check<'a>(prog: &'a Program, src: &Source) -> Result<Ctx<'a>, Vec<Diag>> {
    let mut c = Checker {
        ctx: Ctx { types: HashMap::new(), fns: HashMap::new(), variants: HashMap::new() },
        src_text: src.text,
        diags: Vec::new(),
        scopes: Vec::new(),
        needs: Vec::new(),
    };
    c.collect(prog);
    for item in &prog.items {
        match item {
            Item::Type(d) => c.type_decl(d),
            Item::Schema(d) => c.schema(d),
            Item::Enum(d) => c.enum_decl(d),
            Item::Fn(d) => c.fn_decl(d),
            Item::Test(t) => c.fn_decl(&t.func),
            Item::Cpp { .. } => {}
        }
    }
    if c.diags.is_empty() {
        Ok(c.ctx)
    } else {
        c.diags.sort_by_key(|d| d.span.lo);
        Err(c.diags)
    }
}

struct Checker<'a, 's> {
    ctx: Ctx<'a>,
    src_text: &'s str,
    diags: Vec<Diag>,
    /// Local variables and their types, when known.
    scopes: Vec<HashMap<String, Option<Type>>>,
    needs: Vec<Need>,
}

impl<'a, 's> Checker<'a, 's> {
    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diag::new(span, msg));
    }

    fn text(&self, span: Span) -> &'s str {
        &self.src_text[span.lo..span.hi]
    }

    fn collect(&mut self, prog: &'a Program) {
        let mut seen: HashSet<&str> = HashSet::new();
        for item in &prog.items {
            let name = match item {
                Item::Type(d) => &d.name,
                Item::Schema(d) => &d.name,
                Item::Enum(d) => &d.name,
                Item::Fn(d) => &d.name,
                Item::Test(_) | Item::Cpp { .. } => continue,
            };
            if !seen.insert(&name.text) {
                self.err(name.span, format!("`{}` はすでに宣言されています", name.text));
                continue;
            }
            if builtin(&name.text).is_some() || Prim::from_name(&name.text).is_some() {
                self.err(name.span, format!("`{}` は組み込みの名前なので使えません", name.text));
            }
            match item {
                Item::Type(d) => {
                    self.ctx.types.insert(&d.name.text, TypeInfo::Refined(d));
                }
                Item::Schema(d) => {
                    self.ctx.types.insert(&d.name.text, TypeInfo::Schema(d));
                }
                Item::Enum(d) => {
                    self.ctx.types.insert(&d.name.text, TypeInfo::Enum(d));
                }
                Item::Fn(d) => {
                    self.ctx.fns.insert(&d.name.text, d);
                }
                Item::Test(_) | Item::Cpp { .. } => {}
            }
        }
    }

    // ------------------------------------------------------------- scopes

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: &str, ty: Option<Type>) {
        self.scopes.last_mut().unwrap().insert(name.to_string(), ty);
    }

    fn lookup(&self, name: &str) -> Option<Type> {
        self.scopes.iter().rev().find_map(|s| s.get(name)).cloned().flatten()
    }

    // -------------------------------------------------------- declarations

    fn value_type(&mut self, t: &Type) {
        match &t.kind {
            TypeKind::Raw(_) => self.err(t.span, "Raw<T> は値の型には使えません（関数の引数で受け取ってください）"),
            TypeKind::Optional(i) | TypeKind::Vec(i) => self.value_type(i),
            _ => {}
        }
    }

    /// `where` (with `self` of type `base`) plus `matches` / `transform` functions.
    fn refinement(&mut self, base: &Type, cond: Option<&Expr>, fns: &[&Name]) {
        if let Some(cond) = cond {
            self.push();
            self.bind("self", Some(base.clone()));
            self.pure_expr(cond, Allow { self_: true, old: false }, "where 条件");
            self.where_expr(cond);
            self.pop();
        }
        for f in fns {
            match self.ctx.fns.get(f.text.as_str()) {
                Some(d) if d.body.is_some() => self.diags.push(
                    Diag::new(
                        f.span,
                        format!("`{}` は C.C. で実装された関数なので matches / transform には使えません", f.text),
                    )
                    .note("組み込み関数か、extern fn で宣言した C++ の関数を使ってください"),
                ),
                Some(d) if d.effects.as_ref().is_none_or(|e| !e.is_empty()) => self.diags.push(
                    Diag::new(f.span, format!("`{}` は純粋な関数ではありません", f.text))
                        .note("effects() を宣言した関数だけを使えます"),
                ),
                Some(_) => {}
                None => match builtin(&f.text) {
                    Some(b) if b.effects.is_empty() => {}
                    Some(_) => self.err(f.span, format!("`{}` は純粋な関数ではありません", f.text)),
                    None => self.err(f.span, format!("関数 `{}` が見つかりません", f.text)),
                },
            }
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

    fn enum_decl(&mut self, d: &'a EnumDecl) {
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

    /// `e` is used as a value of type `t`: a bare name means a variant of that enum.
    fn variant(&mut self, e: &Expr, t: &Type) {
        let inner = match &t.kind {
            TypeKind::Optional(i) => i.as_ref(),
            _ => t,
        };
        let (Some(d), ExprKind::Ident(n)) = (self.ctx.enum_of(inner), &e.kind) else { return };
        if self.scopes.iter().any(|s| s.contains_key(n)) {
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

    fn link_variants(&mut self, a: &Expr, b: &Expr) {
        if let Some(t) = self.type_of(a) {
            self.variant(b, &t);
        }
        if let Some(t) = self.type_of(b) {
            self.variant(a, &t);
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
        self.refinement(&d.base, d.cond.as_ref(), &d.matches.iter().collect::<Vec<_>>());
    }

    fn schema(&mut self, d: &'a SchemaDecl) {
        let mut seen = HashSet::new();
        for f in &d.fields {
            if !seen.insert(f.name.text.as_str()) {
                self.err(f.name.span, format!("フィールド `{}` が重複しています", f.name.text));
            }
            self.value_type(&f.ty);
            let inner = match &f.ty.kind {
                TypeKind::Optional(i) => i.as_ref(),
                _ => &f.ty,
            };
            let fns: Vec<&Name> = f.matches.iter().chain(f.transform.iter()).collect();
            self.refinement(inner, f.cond.as_ref(), &fns);
            if f.transform.is_some() && f.cond.is_none() && f.matches.is_none() && self.ctx.schema(inner).is_some() {
                self.err(f.span, "スキーマ型のフィールドには transform を使えません");
            }
            if let Some(def) = &f.default {
                self.push();
                self.pure_expr(def, NONE, "既定値");
                self.pop();
                let checked_on_parse = self.ctx.refined(inner).is_some_and(|d| literal_fits(def, &d.base));
                if !matches!(def.kind, ExprKind::List(_))
                    && f.cond.is_none()
                    && f.matches.is_none()
                    && !checked_on_parse
                {
                    self.check_assign(def, inner);
                }
            }
        }
        self.push();
        for f in &d.fields {
            self.bind(&f.name.text, Some(f.ty.clone()));
        }
        for inv in &d.invariants {
            self.pure_expr(&inv.cond, NONE, "invariant");
        }
        self.pop();
    }

    fn fn_decl(&mut self, d: &'a FnDecl) {
        let name = d.name.text.as_str();
        if !d.is_contract && !(d.pres.is_empty() && d.posts.is_empty()) {
            let span = d.pres.first().map(|c| c.span).or(d.posts.first().map(|c| c.span)).unwrap();
            self.diags.push(
                Diag::new(span, "pre / post は contract fn にだけ書けます")
                    .note(format!("`contract fn {name}` と宣言してください")),
            );
        }
        if name == "main" {
            let args_only = matches!(d.params.as_slice(), [p] if !p.by_ref && matches!(&p.ty.kind,
                TypeKind::Vec(t) if t.kind == TypeKind::Prim(Prim::String)));
            if !d.params.is_empty() && !args_only {
                self.err(d.name.span, "main の引数は `args: vec<string>` だけにしてください");
            }
            if !matches!(d.ret.as_ref().map(|t| &t.kind), None | Some(TypeKind::Prim(Prim::I32 | Prim::I64))) {
                self.err(d.ret.as_ref().unwrap().span, "main の戻り値は i32 にしてください");
            }
            if d.body.is_none() {
                self.err(d.name.span, "main には本体が必要です");
            }
        }
        let declared: Option<HashSet<&str>> = d.effects.as_ref().map(|list| {
            for e in list {
                if !EFFECTS.contains(&e.text.as_str()) {
                    self.diags.push(
                        Diag::new(e.span, format!("未知の効果 `{}`", e.text))
                            .note(format!("使える効果: {}", EFFECTS.join(", "))),
                    );
                }
            }
            list.iter().map(|e| e.text.as_str()).collect()
        });

        self.push();
        for p in &d.params {
            self.value_type_or_raw(&p.ty);
            self.bind(&p.name.text, Some(p.ty.clone()));
        }
        for c in &d.pres {
            self.pure_expr(&c.cond, NONE, "pre 条件");
        }
        for c in &d.posts {
            self.push();
            if let Some(b) = &c.binder {
                match &d.ret {
                    Some(t) => self.bind(&b.text, Some(t.clone())),
                    None => self.err(b.span, "戻り値のない関数の post では結果を受け取れません"),
                }
            }
            self.pure_expr(&c.cond, Allow { self_: false, old: true }, "post 条件");
            self.pop();
        }
        if let Some(body) = &d.body {
            self.needs.clear();
            self.block(body, d);
            let needs = std::mem::take(&mut self.needs);
            if let Some(declared) = declared {
                for n in needs {
                    let missing: Vec<&str> = n.effects.iter().copied().filter(|e| !declared.contains(e)).collect();
                    if missing.is_empty() {
                        continue;
                    }
                    let decl =
                        d.effects.as_ref().unwrap().iter().map(|e| e.text.as_str()).collect::<Vec<_>>().join(", ");
                    let mut diag = Diag::new(
                        n.span,
                        format!(
                            "{}には効果 {} が必要ですが、`{name}` は effects({decl}) しか宣言していません",
                            n.what,
                            effects_str(missing.iter().copied())
                        ),
                    );
                    if let Some(note) = n.note {
                        diag = diag.note(note);
                    }
                    self.diags.push(diag);
                }
            }
        }
        self.pop();
    }

    fn value_type_or_raw(&mut self, t: &Type) {
        if !matches!(t.kind, TypeKind::Raw(_)) {
            self.value_type(t);
        }
    }

    // ------------------------------------------------------------ statements

    fn block(&mut self, b: &'a Block, f: &'a FnDecl) {
        self.push();
        for s in &b.stmts {
            self.stmt(s, f);
        }
        self.pop();
    }

    /// Expression in statement position, where a trailing `?` is allowed.
    fn top_expr(&mut self, e: &Expr) {
        if let ExprKind::Try(inner) = &e.kind {
            if self.type_of(inner).is_some_and(|t| !matches!(t.kind, TypeKind::Result(..))) {
                self.diags.push(
                    Diag::new(e.span, "`?` を使えるのは T::parse / T::try_from の結果だけです")
                        .note("T? の値には match の some / none を使ってください"),
                );
            }
            self.needs.push(Need {
                span: e.span,
                what: "`?`（失敗すると例外を送出します）".into(),
                effects: vec!["throw"],
                note: None,
            });
            self.expr(inner, NONE);
        } else {
            self.expr(e, NONE);
        }
    }

    fn stmt(&mut self, s: &'a Stmt, f: &'a FnDecl) {
        match &s.kind {
            StmtKind::Let { name, ty, init } => {
                self.top_expr(init);
                if let Some(t) = ty {
                    self.value_type_or_raw(t);
                    self.check_assign(init, t);
                }
                let t = ty.clone().or_else(|| self.type_of(init));
                self.bind(name, t);
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target, NONE);
                self.expr(value, NONE);
            }
            StmtKind::Expr(e) => self.top_expr(e),
            StmtKind::Return(value) => match (value, &f.ret) {
                (Some(v), Some(t)) => {
                    self.top_expr(v);
                    self.check_assign(v, t);
                }
                (Some(v), None) if f.name.text != "main" => {
                    self.expr(v, NONE);
                    self.err(v.span, format!("`{}` は値を返さない関数です", f.name.text));
                }
                (None, Some(_)) => self.err(s.span, "戻り値が必要です"),
                _ => {}
            },
            StmtKind::If { cond, then, els } => {
                self.expr(cond, NONE);
                self.block(then, f);
                if let Some(b) = els {
                    self.block(b, f);
                }
            }
            StmtKind::For { var, iter, body } => {
                let var_ty = match iter {
                    ForIter::Range(a, b) => {
                        self.expr(a, NONE);
                        self.expr(b, NONE);
                        Some(Type { kind: TypeKind::Prim(Prim::I64), span: a.span })
                    }
                    ForIter::Each(e) => {
                        self.expr(e, NONE);
                        match self.type_of(e).map(|t| t.kind) {
                            Some(TypeKind::Vec(t)) => Some(*t),
                            _ => None,
                        }
                    }
                };
                self.push();
                self.bind(var, var_ty);
                self.block(body, f);
                self.pop();
            }
            StmtKind::While { cond, body } => {
                self.expr(cond, NONE);
                self.block(body, f);
            }
            StmtKind::Block(b) => self.block(b, f),
            StmtKind::Match { subject, arms } => self.match_stmt(subject, arms, s.span, f),
            StmtKind::Try { body, var, handler } => {
                // Whatever the body throws is caught here, so `throw` does not reach the function.
                let saved = std::mem::take(&mut self.needs);
                self.block(body, f);
                let inner = std::mem::replace(&mut self.needs, saved);
                for mut n in inner {
                    n.effects.retain(|e| *e != "throw");
                    if !n.effects.is_empty() {
                        self.needs.push(n);
                    }
                }
                self.push();
                self.bind(&var.text, Some(Type { kind: TypeKind::Prim(Prim::String), span: var.span }));
                self.block(handler, f);
                self.pop();
            }
            StmtKind::With { effect, handler, entries, body } => {
                if effect.text != "io" {
                    self.err(effect.span, format!("効果 `{}` は差し替えられません（今は io だけです）", effect.text));
                }
                if handler.text != "MemoryFs" {
                    self.err(
                        handler.span,
                        format!("ハンドラ `{}` はありません（今は MemoryFs だけです）", handler.text),
                    );
                }
                for (k, v) in entries {
                    self.expr(k, NONE);
                    self.expr(v, NONE);
                }
                self.block(body, f);
            }
            StmtKind::Expect(cond) => self.expr(cond, NONE),
            StmtKind::Cpp(_) => self.needs.push(Need {
                span: Span { lo: s.span.lo, hi: s.span.lo + 3 },
                what: "cpp ブロック".into(),
                effects: vec!["unsafe"],
                note: Some("cpp ブロックの中身は検査できないため unsafe 扱いです".into()),
            }),
        }
    }

    fn match_stmt(&mut self, subject: &'a Expr, arms: &'a [MatchArm], span: Span, f: &'a FnDecl) {
        self.expr(subject, NONE);
        let subject_ty = self.type_of(subject);
        let has = |k: ArmKind| arms.iter().filter(|a| a.kind == k).count();
        let result_arms = has(ArmKind::Ok) + has(ArmKind::Err) > 0;
        let option_arms = has(ArmKind::Some) + has(ArmKind::None) > 0;
        if result_arms && option_arms {
            self.err(span, "ok / err と some / none を同じ match に混ぜられません");
        } else if !result_arms && !option_arms {
            self.err(span, "match に分岐がありません");
        } else {
            let (a, b) = if result_arms { (ArmKind::Ok, ArmKind::Err) } else { (ArmKind::Some, ArmKind::None) };
            let label = |k| match k {
                ArmKind::Ok => "ok",
                ArmKind::Err => "err",
                ArmKind::Some => "some",
                ArmKind::None => "none",
            };
            for k in [a, b] {
                match has(k) {
                    1 => {}
                    0 => self.err(span, format!("match に {} の分岐がありません", label(k))),
                    _ => self.err(span, format!("match の {} の分岐が重複しています", label(k))),
                }
            }
            let fits = match subject_ty.as_ref().map(|t| &t.kind) {
                None => true,
                Some(TypeKind::Result(..)) => result_arms,
                Some(TypeKind::Optional(_)) => option_arms,
                Some(_) => false,
            };
            if !fits {
                let shown = type_str(subject_ty.as_ref().unwrap());
                let arms = if result_arms { "ok / err" } else { "some / none" };
                self.err(subject.span, format!("{shown} 型の値は {arms} で match できません"));
            }
        }
        for arm in arms {
            let ty = match (arm.kind, subject_ty.as_ref().map(|t| &t.kind)) {
                (ArmKind::Ok, Some(TypeKind::Result(t, _))) => Some((**t).clone()),
                (ArmKind::Err, Some(TypeKind::Result(_, e))) => Some((**e).clone()),
                (ArmKind::Some, Some(TypeKind::Optional(t))) => Some((**t).clone()),
                _ => None,
            };
            self.push();
            if let Some(b) = &arm.binder {
                self.bind(&b.text, ty);
            }
            self.block(&arm.body, f);
            self.pop();
        }
    }

    // ----------------------------------------------------------- expressions

    /// A condition or default value: every effect is an error.
    fn pure_expr(&mut self, e: &Expr, allow: Allow, what: &str) {
        let saved = std::mem::take(&mut self.needs);
        self.expr(e, allow);
        let needs = std::mem::replace(&mut self.needs, saved);
        for n in needs {
            self.diags.push(Diag::new(
                n.span,
                format!(
                    "{what}は純粋でなければなりませんが、{}は効果 {} を持ちます",
                    n.what,
                    effects_str(n.effects.iter().copied())
                ),
            ));
        }
    }

    fn expr(&mut self, e: &Expr, allow: Allow) {
        match &e.kind {
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str { .. }
            | ExprKind::Bool(_)
            | ExprKind::None
            | ExprKind::Ident(_) => {}
            ExprKind::Path(t, m) => {
                if let Some(TypeInfo::Enum(d)) = self.ctx.types.get(t.as_str()) {
                    if !d.variants.iter().any(|v| v.text == *m) {
                        self.err(e.span, format!("`{m}` は {t} の値ではありません"));
                    }
                }
            }
            ExprKind::SelfValue => {
                if !allow.self_ {
                    self.err(e.span, "self は where 条件の中でだけ使えます");
                }
            }
            ExprKind::List(items) => items.iter().for_each(|i| self.expr(i, allow)),
            ExprKind::Field(obj, _) => self.expr(obj, allow),
            ExprKind::Index(a, i) => {
                self.expr(a, allow);
                self.expr(i, allow);
            }
            ExprKind::Unary(_, a) => self.expr(a, allow),
            ExprKind::Binary(_, a, b) => {
                self.expr(a, allow);
                self.expr(b, allow);
            }
            ExprKind::Compare(first, rest) => {
                self.expr(first, allow);
                rest.iter().for_each(|(_, r)| self.expr(r, allow));
                let mut lhs = first.as_ref();
                for (_, r) in rest {
                    self.link_variants(lhs, r);
                    lhs = r;
                }
            }
            ExprKind::Old(inner) => {
                if !allow.old {
                    self.err(e.span, "old() は post 条件の中でだけ使えます");
                }
                self.expr(inner, Allow { old: false, ..allow });
            }
            ExprKind::Try(_) => self.diags.push(
                Diag::new(e.span, "`?` はここでは使えません")
                    .note("`?` は let の右辺・return・式文の末尾にだけ書けます"),
            ),
            ExprKind::Call(callee, args) => {
                self.call(e, callee, args);
                if let ExprKind::Field(obj, _) = &callee.kind {
                    self.expr(obj, allow);
                }
                args.iter().for_each(|a| self.expr(a, allow));
            }
        }
    }

    fn call(&mut self, e: &Expr, callee: &Expr, args: &[Expr]) {
        let unsafe_need =
            |what: String, note: String| Need { span: e.span, what, effects: vec!["unsafe"], note: Some(note) };
        match &callee.kind {
            ExprKind::Ident(n) => {
                if let Some(b) = builtin(n) {
                    if !b.effects.is_empty() {
                        self.needs.push(Need {
                            span: e.span,
                            what: format!("`{n}` の呼び出し"),
                            effects: b.effects.to_vec(),
                            note: None,
                        });
                    }
                    return;
                }
                let Some(&d) = self.ctx.fns.get(n.as_str()) else {
                    if self.lookup(n).is_none() {
                        self.needs.push(unsafe_need(
                            format!("`{n}` の呼び出し"),
                            format!("C.C. で宣言されていない関数は unsafe 扱いです。`extern fn {n}(...) effects(...);` で効果を宣言できます"),
                        ));
                    }
                    return;
                };
                match &d.effects {
                    Some(list) => {
                        let effects: Vec<&'static str> =
                            list.iter().filter_map(|x| EFFECTS.iter().copied().find(|k| *k == x.text)).collect();
                        if !effects.is_empty() {
                            self.needs.push(Need {
                                span: e.span,
                                what: format!("`{n}` の呼び出し"),
                                effects,
                                note: None,
                            });
                        }
                    }
                    None => self.needs.push(unsafe_need(
                        format!("`{n}` の呼び出し"),
                        format!(
                            "`{n}` は効果を宣言していないため unsafe 扱いです。`{n}` に effects(...) を付けてください"
                        ),
                    )),
                }
                if args.len() != d.params.len() {
                    self.err(
                        e.span,
                        format!("`{n}` の引数は {} 個ですが、{} 個渡しています", d.params.len(), args.len()),
                    );
                    return;
                }
                for (a, p) in args.iter().zip(&d.params) {
                    self.check_assign(a, &p.ty);
                }
            }
            ExprKind::Path(t, m) => {
                if self.generated_result(t, m).is_none() {
                    self.needs.push(unsafe_need(
                        format!("`{t}::{m}` の呼び出し"),
                        "C.C. で宣言されていない関数は unsafe 扱いです".into(),
                    ));
                }
            }
            ExprKind::Field(obj, m) => {
                // What the compiler generated is known to be pure. Anything else is C++
                // whose effects (a throwing `at()`, say) nothing here can see.
                let generated = self.type_of(obj).is_some_and(|t| match m.as_str() {
                    "to_json" => self.ctx.schema(&t).is_some(),
                    "get" => self.ctx.refined(&t).is_some(),
                    _ => false,
                });
                if !generated {
                    self.needs.push(unsafe_need(
                        format!("メソッド `{m}` の呼び出し"),
                        "C++ のメソッドは効果が分からないため unsafe 扱いです。結果の取り出しには match を使うか、extern fn で包んでください"
                            .into(),
                    ));
                }
            }
            _ => {}
        }
    }

    /// The type of `T::m(...)` when the compiler generates it.
    fn generated_result(&self, t: &str, m: &str) -> Option<TypeKind> {
        let err = match (self.ctx.types.get(t)?, m) {
            (TypeInfo::Schema(_), "parse") => "ParseError",
            (TypeInfo::Refined(_), "try_from") => "RefineError",
            _ => return None,
        };
        let named = |n: &str| Box::new(Type { kind: TypeKind::Named(n.to_string()), span: Span::default() });
        Some(TypeKind::Result(named(t), named(err)))
    }

    // --------------------------------------------------------------- typing

    /// The type of an expression when it is cheap to know. `None` means unknown.
    fn type_of(&self, e: &Expr) -> Option<Type> {
        let mk = |kind| Some(Type { kind, span: e.span });
        match &e.kind {
            ExprKind::Int(_) => mk(TypeKind::Prim(Prim::I64)),
            ExprKind::Float(_) => mk(TypeKind::Prim(Prim::F64)),
            ExprKind::Str { .. } => mk(TypeKind::Prim(Prim::String)),
            ExprKind::Bool(_) => mk(TypeKind::Prim(Prim::Bool)),
            ExprKind::Ident(n) => self.lookup(n),
            ExprKind::Call(callee, _) => match &callee.kind {
                ExprKind::Ident(n) => match builtin(n) {
                    Some(b) => b.ret.and_then(mk),
                    None => self.ctx.fns.get(n.as_str()).and_then(|d| d.ret.clone()),
                },
                ExprKind::Path(t, m) => self.generated_result(t, m).and_then(mk),
                _ => None,
            },
            ExprKind::Path(t, _) if matches!(self.ctx.types.get(t.as_str()), Some(TypeInfo::Enum(_))) => {
                mk(TypeKind::Named(t.clone()))
            }
            ExprKind::Try(inner) => match self.type_of(inner)?.kind {
                TypeKind::Result(t, _) => Some(*t),
                _ => None,
            },
            ExprKind::Binary(op, a, _) if !matches!(op, BinOp::And | BinOp::Or) => {
                self.type_of(a).filter(|t| matches!(t.kind, TypeKind::Prim(_)))
            }
            ExprKind::Unary(UnOp::Neg, a) => self.type_of(a),
            ExprKind::Field(obj, f) => {
                let t = self.type_of(obj)?;
                let s = self.ctx.schema(&t)?;
                s.fields.iter().find(|x| &x.name.text == f).map(|x| x.ty.clone())
            }
            _ => None,
        }
    }

    /// `e` is used where a value of type `target` is expected.
    fn check_assign(&mut self, e: &Expr, target: &Type) {
        self.variant(e, target);
        let target_s = type_str(target);
        let actual = self.type_of(e);
        if matches!(actual.as_ref().map(|t| &t.kind), Some(TypeKind::Result(..))) {
            self.diags.push(
                Diag::new(e.span, format!("失敗するかもしれない結果を {target_s} として使えません"))
                    .note("`?` で失敗を送出するか、match の ok / err で分けてください"),
            );
            return;
        }
        if let Some(TypeKind::Raw(_)) = actual.as_ref().map(|t| &t.kind) {
            if !matches!(target.kind, TypeKind::Raw(_)) {
                let parse = if self.ctx.schema(target).is_some() {
                    format!("{target_s}::parse(...)")
                } else {
                    "スキーマの parse(...)".into()
                };
                self.diags.push(
                    Diag::new(
                        e.span,
                        format!("未検証の {} を {target_s} として使えません", type_str(actual.as_ref().unwrap())),
                    )
                    .note(format!("先に {parse} で検証してください（ここが信頼境界です）")),
                );
            }
            return;
        }
        if self.ctx.refined(target).is_none() {
            return;
        }
        if is_literal(e) {
            if !(integral_literal(e) && self.ctx.literal_refined(target).is_some()) {
                self.diags.push(
                    Diag::new(
                        e.span,
                        format!("リテラル {} が {target_s} の条件を満たすかは静的に検査できません", self.text(e.span)),
                    )
                    .note(format!("{target_s}::try_from({})? で明示的に変換してください", self.text(e.span))),
                );
            }
            return;
        }
        if let Some(t) = actual {
            if t != *target {
                self.diags.push(
                    Diag::new(e.span, format!("{} 型の値を {target_s} に暗黙変換できません", type_str(&t))).note(
                        format!(
                        "条件は実行時にしか確かめられないため、{target_s}::try_from(...)? で明示的に変換してください"
                    ),
                    ),
                );
            }
        }
    }
}
