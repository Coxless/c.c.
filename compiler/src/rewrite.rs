//! The transform from .c.c to C++. It walks the C++ tokens of the file looking for
//! extensions, and copies everything else through character for character. `#line`
//! directives and padding keep every copied token on its original line and column.

use std::collections::HashSet;

use crate::ast::*;
use crate::check::{self, EFFECTS};
use crate::diag::{Diag, Source};
use crate::emit::{cpp_str, is_cpp_keyword, Emitter};
use crate::lexer::{self, Tok};
use crate::parser;
use crate::scan::{self, K, T};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Output to build and run.
    Build,
    /// Output for the effect check under Clang: contract checks keep their conditions,
    /// which are part of what a function does, but not the reporting around them.
    Check,
}

pub struct Output {
    pub cpp: String,
    pub has_tests: bool,
    pub uses_effects: bool,
    /// The `.c.h` files this file includes, as written.
    pub includes: Vec<String>,
}

/// A replacement of `src[lo..hi]`. A block is generated code on lines of its own;
/// the text after it continues on a new line, restored to its original position.
struct Edit {
    lo: usize,
    hi: usize,
    text: String,
    block: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Scope {
    Namespace,
    Class,
    /// A function body, or anything else that is not a list of declarations.
    Block,
}

struct ParamInfo {
    name: String,
    /// The declaration without its default value.
    text: String,
    /// ` = value`, or empty.
    default: String,
    by_value: bool,
    rvalue: bool,
}

struct Clause {
    is_post: bool,
    binder: Option<String>,
    /// Token range of the condition.
    cond: (usize, usize),
    label: Option<String>,
    lo: usize,
}

struct CppEnum {
    decl: EnumDecl,
    /// Where the declaration ends, after its `;`.
    end: usize,
    has_values: bool,
}

struct Rw<'a> {
    src: &'a str,
    source: &'a Source<'a>,
    toks: Vec<T>,
    file: String,
    mode: Mode,
    is_header: bool,
    macros: HashSet<&'a str>,
    edits: Vec<Edit>,
    diags: Vec<Diag>,
    types: Vec<TypeDecl>,
    schemas: Vec<SchemaDecl>,
    inline_enums: Vec<(usize, EnumDecl)>,
    cpp_enums: Vec<CppEnum>,
    tests: Vec<String>,
    includes: Vec<String>,
    strict: bool,
    uses_effects: bool,
    uses_contracts: bool,
    has_extensions: bool,
    has_main: bool,
}

pub fn transform(name: &str, text: &str, mode: Mode, is_header: bool) -> Result<Output, String> {
    let source = Source::new(name, text);
    let toks = scan::scan(text);
    let file = name.rsplit('/').next().unwrap_or(name).to_string();
    let mut r = Rw {
        src: text,
        source: &source,
        toks,
        file,
        mode,
        is_header,
        macros: HashSet::new(),
        edits: Vec::new(),
        diags: Vec::new(),
        types: Vec::new(),
        schemas: Vec::new(),
        inline_enums: Vec::new(),
        cpp_enums: Vec::new(),
        tests: Vec::new(),
        includes: Vec::new(),
        strict: false,
        uses_effects: false,
        uses_contracts: false,
        has_extensions: false,
        has_main: false,
    };
    r.directives();
    r.effect_attributes();
    r.walk();
    r.declarations();
    if !r.diags.is_empty() {
        r.diags.sort_by_key(|d| d.span.lo);
        return Err(r.diags.iter().map(|d| source.render(d)).collect::<Vec<_>>().join("\n"));
    }
    Ok(r.assemble())
}

impl<'a> Rw<'a> {
    fn text(&self, i: usize) -> &'a str {
        self.toks.get(i).map_or("", |t| &self.src[t.lo..t.hi])
    }

    fn is(&self, i: usize, word: &str) -> bool {
        self.text(i) == word
    }

    fn kind(&self, i: usize) -> Option<K> {
        self.toks.get(i).map(|t| t.k)
    }

    fn is_ident(&self, i: usize) -> bool {
        self.kind(i) == Some(K::Ident)
    }

    /// An extension keyword, unless the file has made the word mean something else.
    fn keyword(&self, i: usize, word: &str) -> bool {
        self.is(i, word) && self.is_ident(i) && !self.macros.contains(word)
    }

    fn span(&self, i: usize) -> Span {
        Span { lo: self.toks[i].lo, hi: self.toks[i].hi }
    }

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        self.diags.push(Diag::new(span, msg));
    }

    fn inline(&mut self, lo: usize, hi: usize, text: String) {
        self.edits.push(Edit { lo, hi, text, block: false });
    }

    fn block(&mut self, lo: usize, hi: usize, text: String) {
        self.edits.push(Edit { lo, hi, text, block: true });
    }

    fn line_directive(&self, pos: usize) -> String {
        format!("#line {} {}\n", self.source.line(pos), cpp_str(&self.file))
    }

    // ------------------------------------------------------------ brackets

    /// The token closing the bracket at `i`. Only the first branch of an `#if` counts,
    /// the way a compiler taking that branch would see it.
    fn matching(&self, i: usize) -> Option<usize> {
        let (open, close) = match self.text(i) {
            "(" => ("(", ")"),
            "{" => ("{", "}"),
            "[" => ("[", "]"),
            _ => return None,
        };
        let mut depth = 0usize;
        let mut j = i;
        while j < self.toks.len() {
            let t = self.toks[j];
            if t.k == K::Directive {
                if matches!(directive_word(self.text(j)), "else" | "elif" | "elifdef" | "elifndef") {
                    j = self.skip_to_endif(j);
                    continue;
                }
            } else if t.k == K::Punct {
                let w = self.text(j);
                if w == open {
                    depth += 1;
                } else if w == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
            }
            j += 1;
        }
        None
    }

    fn skip_to_endif(&self, mut j: usize) -> usize {
        let mut nest = 0;
        j += 1;
        while j < self.toks.len() {
            if self.toks[j].k == K::Directive {
                match directive_word(self.text(j)) {
                    "if" | "ifdef" | "ifndef" => nest += 1,
                    "endif" if nest == 0 => return j,
                    "endif" => nest -= 1,
                    _ => {}
                }
            }
            j += 1;
        }
        j
    }

    /// The `>` closing the `<` at `i`, when that `<` plausibly opens template arguments.
    fn matching_angle(&self, i: usize) -> Option<usize> {
        let mut depth = 0;
        let mut j = i;
        while j < self.toks.len() {
            match self.text(j) {
                "<" => depth += 1,
                ">" => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(j);
                    }
                }
                "(" | "[" | "{" => j = self.matching(j)?,
                ";" | ")" | "]" | "}" => return None,
                _ => {}
            }
            j += 1;
        }
        None
    }

    // ---------------------------------------------------------- directives

    fn directives(&mut self) {
        for i in 0..self.toks.len() {
            if self.toks[i].k != K::Directive {
                continue;
            }
            let t = self.toks[i];
            let line = self.text(i);
            let words: Vec<&str> = line[1..].split_whitespace().collect();
            match words.as_slice() {
                ["define", name, ..] => {
                    let end = name.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(name.len());
                    self.macros.insert(&name[..end]);
                }
                ["pragma", "cc", "strict"] => {
                    if self.is_header {
                        self.err(self.span(i), "#pragma cc strict は .c.h には書けません");
                    }
                    self.strict = true;
                    self.inline(t.lo, t.hi, "CC_STRICT".into());
                }
                ["pragma", "cc", rest @ ..] => {
                    self.err(self.span(i), format!("未知の指定 `#pragma cc {}`", rest.join(" ")));
                }
                ["include", path] if path.len() > 7 && path.starts_with('"') && path.ends_with(".c.h\"") => {
                    let inner = &path[1..path.len() - 1];
                    let stem = inner.rsplit('/').next().unwrap().trim_end_matches(".c.h");
                    self.includes.push(inner.to_string());
                    self.inline(t.lo, t.hi, format!("#include \"{stem}.h\""));
                }
                _ => {}
            }
        }
    }

    // ------------------------------------------------- [[cc::effects(...)]]

    /// `cc::effects(io)` inside `[[ ]]` becomes a macro: Clang's `annotate` for the
    /// effect check, nothing for g++, which would warn about an unknown attribute.
    fn effect_attributes(&mut self) {
        for i in 0..self.toks.len() {
            if !(self.is(i, "cc") && self.is(i + 1, "::") && self.is(i + 2, "effects") && self.is(i + 3, "(")) {
                continue;
            }
            if !(i >= 1 && (self.is(i - 1, "[") || self.is(i - 1, ","))) {
                continue;
            }
            let Some(close) = self.matching(i + 3) else { continue };
            let mut names = Vec::new();
            for j in i + 4..close {
                if self.is(j, ",") {
                    continue;
                }
                let name = self.text(j);
                if !EFFECTS.contains(&name) {
                    let note = format!("使える効果: {}", EFFECTS.join(", "));
                    self.diags.push(Diag::new(self.span(j), format!("未知の効果 `{name}`")).note(note));
                }
                names.push(name);
            }
            self.uses_effects = true;
            let (lo, hi) = (self.toks[i].lo, self.toks[close].hi);
            self.inline(lo, hi, format!("CC_EFFECTS({})", cpp_str(&names.join(","))));
        }
    }

    // ---------------------------------------------------------------- walk

    fn walk(&mut self) {
        let mut scopes: Vec<Scope> = Vec::new();
        let mut if_stack: Vec<Vec<Scope>> = Vec::new();
        // The token where the current declaration at namespace scope starts.
        let mut start = 0;
        let mut i = 0;
        while i < self.toks.len() {
            let in_namespace = scopes.iter().all(|s| *s == Scope::Namespace);
            let in_block = scopes.contains(&Scope::Block);
            match self.toks[i].k {
                K::Directive => {
                    match directive_word(self.text(i)) {
                        "if" | "ifdef" | "ifndef" => if_stack.push(scopes.clone()),
                        "else" | "elif" | "elifdef" | "elifndef" => {
                            if let Some(saved) = if_stack.last() {
                                scopes = saved.clone();
                            }
                        }
                        "endif" => {
                            if_stack.pop();
                        }
                        _ => {}
                    }
                    if !in_block {
                        start = i + 1;
                    }
                    i += 1;
                }
                K::Punct => {
                    match self.text(i) {
                        "{" => {
                            let scope = if in_block { Scope::Block } else { self.scope_opened(start, i, in_namespace) };
                            scopes.push(scope);
                            if scope != Scope::Block {
                                start = i + 1;
                            }
                        }
                        "}" => {
                            scopes.pop();
                            if !scopes.contains(&Scope::Block) {
                                start = i + 1;
                            }
                        }
                        ";" if !in_block => start = i + 1,
                        _ => {}
                    }
                    i += 1;
                }
                K::Ident => {
                    let at_start = in_namespace && i == start;
                    let boundary = i == 0 || matches!(self.text(i - 1), ";" | "{" | "}");
                    if let Some(next) = self.extension(i, at_start, boundary, scopes.is_empty()) {
                        i = next;
                        if in_namespace {
                            start = i;
                        }
                    } else if !in_block && self.is_clause_start(i) {
                        i = self.contract(i, start, in_namespace);
                    } else {
                        if scopes.is_empty() && self.is(i, "main") && self.is(i + 1, "(") {
                            self.has_main = true;
                        }
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
    }

    fn scope_opened(&self, start: usize, open: usize, in_namespace: bool) -> Scope {
        let has = |w: &str| (start..open).any(|j| self.is(j, w));
        let linkage = start + 2 == open && self.is(start, "extern") && self.kind(start + 1) == Some(K::Str);
        if in_namespace && (has("namespace") || linkage) {
            Scope::Namespace
        } else if !has("(") && (has("class") || has("struct") || has("union")) {
            Scope::Class
        } else {
            Scope::Block
        }
    }

    /// An extension at `i`: handles it and returns the token after it.
    fn extension(&mut self, i: usize, at_start: bool, boundary: bool, top_level: bool) -> Option<usize> {
        let found = self.type_end(i).map(|e| ("type", e)).or_else(|| self.schema_end(i).map(|e| ("schema", e)));
        if let Some((what, end)) = found {
            if !at_start {
                if boundary {
                    self.err(self.span(i), format!("{what} は名前空間のスコープにだけ書けます"));
                    return Some(end + 1);
                }
                return None;
            }
            if what == "type" {
                self.parse_type(i, end);
            } else {
                self.parse_schema(i, end);
            }
            return Some(end + 1);
        }
        if at_start && self.is(i, "enum") {
            self.cpp_enum(i);
            return None;
        }
        if self.keyword(i, "test") && self.kind(i + 1) == Some(K::Str) && self.is(i + 2, "{") {
            if !(at_start && top_level) {
                if boundary {
                    self.err(self.span(i), "test はファイルのトップレベルにだけ書けます");
                    return Some(self.matching(i + 2).unwrap_or(i + 2) + 1);
                }
                return None;
            }
            return Some(self.test_block(i));
        }
        None
    }

    // -------------------------------------------------------------- `type`

    /// The `;` of `type Name = ...;`. Without `where` or `matches` this is not an
    /// extension: `type x = 1;` is valid C++ when `type` names a type.
    fn type_end(&self, i: usize) -> Option<usize> {
        if !(self.keyword(i, "type") && self.is_ident(i + 1) && self.is(i + 2, "=")) {
            return None;
        }
        let mut j = i + 3;
        let mut found = false;
        while j < self.toks.len() {
            match self.text(j) {
                ";" => return found.then_some(j),
                "where" | "matches" => found = true,
                "(" | "[" | "{" => j = self.matching(j)?,
                ")" | "]" | "}" => return None,
                _ => {}
            }
            j += 1;
        }
        None
    }

    fn parse_type(&mut self, i: usize, end: usize) {
        let (lo, hi) = (self.toks[i].lo, self.toks[end].hi);
        match lexer::lex(self.src, lo, hi).and_then(parser::parse_type) {
            Ok(d) => self.types.push(d),
            Err(d) => self.diags.push(d),
        }
    }

    // ------------------------------------------------------------ `schema`

    /// The last token of `[open] schema Name { field: ...`. `schema x { };` and
    /// `schema x{1};` stay C++: a field or an `invariant` is what makes it an extension.
    fn schema_end(&self, i: usize) -> Option<usize> {
        let word = if self.keyword(i, "open") && self.keyword(i + 1, "schema") { i + 1 } else { i };
        if !(self.keyword(word, "schema") && self.is_ident(word + 1) && self.is(word + 2, "{")) {
            return None;
        }
        let first = word + 3;
        let field = self.is_ident(first) && self.is(first + 1, ":");
        let invariant = self.is(first, "invariant") && self.is(first + 1, "(");
        if !(field || invariant) {
            return None;
        }
        let close = self.matching(word + 2)?;
        Some(if self.is(close + 1, ";") { close + 1 } else { close })
    }

    fn parse_schema(&mut self, i: usize, end: usize) {
        let (lo, hi) = (self.toks[i].lo, self.toks[end].hi);
        let hi_body = if self.is(end, ";") { self.toks[end - 1].hi } else { hi };
        match lexer::lex(self.src, lo, hi_body).and_then(parser::parse_schema) {
            Ok((mut d, enums)) => {
                d.span.hi = hi;
                let at = d.span.lo;
                self.inline_enums.extend(enums.into_iter().map(|e| (at, e)));
                self.schemas.push(d);
            }
            Err(d) => self.diags.push(d),
        }
    }

    // -------------------------------------------------------------- `enum`

    /// `enum [class] Name [: base] { a, b };`, in case a schema turns out to use it.
    fn cpp_enum(&mut self, i: usize) {
        let mut j = i + 1;
        if self.is(j, "class") || self.is(j, "struct") {
            j += 1;
        }
        if !self.is_ident(j) {
            return;
        }
        let name = Name { text: self.text(j).to_string(), span: self.span(j) };
        j += 1;
        while j < self.toks.len() && !matches!(self.text(j), "{" | ";" | "(" | "=") {
            j += 1;
        }
        if !self.is(j, "{") {
            return;
        }
        let Some(close) = self.matching(j) else { return };
        if !self.is(close + 1, ";") {
            return;
        }
        let mut variants = Vec::new();
        let mut has_values = false;
        let mut k = j + 1;
        while k < close {
            if self.is_ident(k) {
                variants.push(Name { text: self.text(k).to_string(), span: self.span(k) });
            }
            k += 1;
            while k < close && !self.is(k, ",") {
                if self.is(k, "=") {
                    has_values = true;
                }
                k = match self.text(k) {
                    "(" | "{" | "[" => self.matching(k).unwrap_or(k) + 1,
                    _ => k + 1,
                };
            }
            k += 1;
        }
        let decl = EnumDecl { name, variants, span: self.span(i) };
        self.cpp_enums.push(CppEnum { decl, end: self.toks[close + 1].hi, has_values });
    }

    // -------------------------------------------------------------- `test`

    fn test_block(&mut self, i: usize) -> usize {
        let Some(close) = self.matching(i + 2) else {
            self.err(self.span(i + 2), "`{` が閉じていません");
            return self.toks.len();
        };
        if self.is_header {
            self.err(self.span(i), "test は .c.h には書けません");
        }
        let label = self.decode_string(i + 1);
        let n = self.tests.len();
        self.tests.push(label);
        self.block(self.toks[i].lo, self.toks[i + 1].hi, format!("#ifdef CC_TEST\nstatic void cc_test_{n}()"));
        let mut j = i + 3;
        while j < close {
            let plain = !(j > 0 && matches!(self.text(j - 1), "." | "->" | "::"));
            if plain && self.is(j, "expect") && self.is(j + 1, "(") {
                if let Some(end) = self.matching(j + 1) {
                    self.expect_call(j, end);
                    j = end + 1;
                    continue;
                }
            }
            j += 1;
        }
        let end = self.toks[close].hi;
        self.block(end, end, "#endif".into());
        close + 1
    }

    fn expect_call(&mut self, word: usize, close: usize) {
        let (lo, hi) = (self.toks[word + 1].hi, self.toks[close].lo);
        let cond = &self.src[lo..hi];
        let values = self.values(word + 2, close, None, &[], 0);
        let text = format!(
            "do {{ if (!({cond})) ::cc::expect_failed({}, std::source_location::current(), {values}); }} while (0)",
            cpp_str(&one_line(cond))
        );
        self.inline(self.toks[word].lo, self.toks[close].hi, text);
    }

    fn decode_string(&self, i: usize) -> String {
        let t = self.toks[i];
        match lexer::lex(self.src, t.lo, t.hi).ok().and_then(|v| v.into_iter().next()) {
            Some(lexer::Token { tok: Tok::Str { value, .. }, .. }) => value,
            _ => self.text(i).trim_matches('"').to_string(),
        }
    }

    // --------------------------------------------------------- `pre`, `post`

    /// `pre(` or `post(` where a clause can be: after a declarator, followed by another
    /// clause or the end of the declaration. Anywhere else it is an ordinary name, as in
    /// `int pre(int x);`. Only called outside function bodies, where `if (a) pre(b);` is.
    fn is_clause_start(&self, i: usize) -> bool {
        if !(self.keyword(i, "pre") || self.keyword(i, "post")) || !self.is(i + 1, "(") || i == 0 {
            return false;
        }
        let closes_declarator = |w: &str| matches!(w, ")" | "noexcept" | "const" | "volatile" | "override" | "final");
        let mut after_declarator = closes_declarator(self.text(i - 1)) || self.is(i - 1, "&");
        // A trailing return type between the parameters and the clause.
        let mut j = i;
        while !after_declarator && j > 0 {
            j -= 1;
            match self.text(j) {
                "->" => {
                    after_declarator = j > 0 && closes_declarator(self.text(j - 1));
                    break;
                }
                ";" | "{" | "}" => break,
                _ => {}
            }
        }
        if !after_declarator {
            return false;
        }
        let Some(close) = self.matching(i + 1) else { return false };
        matches!(self.text(close + 1), "{" | ";" | "pre" | "post")
    }

    /// Handles the clauses starting at `first`. Returns the token after the last clause;
    /// the function body that follows is walked like any other.
    fn contract(&mut self, first: usize, start: usize, in_namespace: bool) -> usize {
        let mut clauses = Vec::new();
        let mut j = first;
        while (self.keyword(j, "pre") || self.keyword(j, "post")) && self.is(j + 1, "(") {
            let Some(close) = self.matching(j + 1) else { break };
            clauses.push(self.clause(j, close));
            j = close + 1;
        }
        let after = j;
        if !in_namespace {
            let d = Diag::new(self.span(first), "pre / post はメンバ関数にはまだ書けません");
            self.diags.push(d.note("今は名前空間のスコープの普通の関数だけに対応しています"));
            return after;
        }
        if !self.is(after, "{") {
            self.err(self.span(first), "pre / post は関数の定義に書いてください（宣言だけの関数には書けません）");
            return after;
        }
        let Some(body_end) = self.matching(after) else { return after };
        let Some(sig) = self.signature(start, first) else { return after };
        self.uses_contracts = true;
        self.function_with_contract(&clauses, start, first, after, body_end, sig);
        after
    }

    fn clause(&mut self, word: usize, close: usize) -> Clause {
        let is_post = self.is(word, "post");
        let mut lo = word + 2;
        let mut binder = None;
        if is_post && self.is_ident(lo) && self.is(lo + 1, ":") {
            binder = Some(self.text(lo).to_string());
            lo += 2;
        }
        let mut hi = close;
        let mut label = None;
        if hi >= lo + 2 && self.kind(hi - 1) == Some(K::Str) && self.is(hi - 2, ",") {
            label = Some(self.decode_string(hi - 1));
            hi -= 2;
        }
        if lo >= hi {
            self.err(self.span(word), "条件がありません");
        }
        Clause { is_post, binder, cond: (lo, hi), label, lo: self.toks[word].lo }
    }

    fn slice(&self, lo: usize, hi: usize) -> &'a str {
        if lo >= hi {
            return "";
        }
        &self.src[self.toks[lo].lo..self.toks[hi - 1].hi]
    }

    fn signature(&mut self, start: usize, clause: usize) -> Option<Signature> {
        let unsupported = |r: &mut Self, at: usize, what: &str| -> Option<Signature> {
            let msg = format!("{what}の pre / post にはまだ対応していません");
            let note = "今は名前空間のスコープの普通の関数だけに対応しています";
            r.diags.push(Diag::new(r.span(at), msg).note(note));
            None
        };
        if self.is(start, "template") {
            return unsupported(self, start, "テンプレート");
        }
        let mut j = start;
        let open = loop {
            if j >= clause {
                self.err(self.span(clause), "pre / post の前に関数の宣言が見つかりません");
                return None;
            }
            match self.text(j) {
                "[" => j = self.matching(j)? + 1,
                "(" => {
                    let prev = if j > start { self.text(j - 1) } else { "" };
                    if matches!(prev, "decltype" | "alignas" | "__attribute__" | "__declspec" | "noexcept") {
                        j = self.matching(j)? + 1;
                    } else {
                        break j;
                    }
                }
                "<" => j = self.matching_angle(j).map_or(j + 1, |c| c + 1),
                _ => j += 1,
            }
        };
        let name = open.checked_sub(1).filter(|n| *n >= start && self.is_ident(*n))?;
        if name > start && matches!(self.text(name - 1), "::" | "operator" | "~") {
            return unsupported(self, name, "メンバ関数や演算子");
        }
        if self.is(name, "main") {
            self.err(self.span(name), "main には pre / post を書けません");
            return None;
        }
        let close = self.matching(open)?;
        let mut k = close + 1;
        let mut tail_noexcept = (k, k);
        let mut trailing = None;
        while k < clause {
            match self.text(k) {
                "noexcept" => {
                    let end = if self.is(k + 1, "(") { self.matching(k + 1)? + 1 } else { k + 1 };
                    tail_noexcept = (k, end);
                    k = end;
                }
                "->" => {
                    trailing = Some((k, clause));
                    k = clause;
                }
                "requires" => return unsupported(self, k, "requires の付いた関数"),
                _ => return unsupported(self, k, "メンバ関数"),
            }
        }
        let params = self.params(open, close)?;
        // The return type, seen without attributes and specifiers.
        let mut ret: Vec<&str> = Vec::new();
        let mut m = start;
        while m < name {
            if self.is(m, "[") {
                m = self.matching(m)? + 1;
                continue;
            }
            let w = self.text(m);
            if !matches!(w, "static" | "inline" | "constexpr" | "extern" | "friend") {
                ret.push(w);
            }
            m += 1;
        }
        if self.is(start, "consteval") || ret.contains(&"consteval") {
            return unsupported(self, start, "consteval 関数");
        }
        if ret.contains(&"auto") && trailing.is_none() {
            self.err(self.span(name), "pre / post を書く関数の戻り値に auto は使えません（型を書いてください）");
            return None;
        }
        let returns_void = match trailing {
            Some((lo, hi)) => self.slice(lo + 1, hi).trim() == "void",
            None => ret == ["void"],
        };
        Some(Signature { name, params, tail_noexcept, trailing, returns_void })
    }

    fn params(&mut self, open: usize, close: usize) -> Option<Vec<ParamInfo>> {
        let mut out = Vec::new();
        let mut lo = open + 1;
        if lo == close || (lo + 1 == close && self.is(lo, "void")) {
            return Some(out);
        }
        let mut j = lo;
        while j <= close {
            let end_here = j == close || self.is(j, ",");
            if end_here {
                out.push(self.param(lo, j)?);
                lo = j + 1;
                j += 1;
                continue;
            }
            j = match self.text(j) {
                "(" | "[" | "{" => self.matching(j)? + 1,
                "<" => self.matching_angle(j).map_or(j + 1, |c| c + 1),
                _ => j + 1,
            };
        }
        Some(out)
    }

    fn param(&mut self, lo: usize, hi: usize) -> Option<ParamInfo> {
        if (lo..hi).any(|i| self.is(i, "...")) {
            self.err(self.span(lo), "可変長引数の pre / post にはまだ対応していません");
            return None;
        }
        // Up to the default value, if there is one.
        let mut end = hi;
        let mut j = lo;
        while j < hi {
            match self.text(j) {
                "=" => {
                    end = j;
                    break;
                }
                "(" | "[" | "{" => j = self.matching(j)?,
                "<" => j = self.matching_angle(j).unwrap_or(j),
                _ => {}
            }
            j += 1;
        }
        let mut name_at = end - 1;
        if self.is(name_at, "]") {
            while name_at > lo && !self.is(name_at, "[") {
                name_at -= 1;
            }
            name_at -= 1;
        }
        if self.is(name_at, ")") {
            // `void (*f)(int)`: the name is inside the first parentheses.
            if let Some(star) = (lo..end).find(|i| self.is(*i, "(") && self.is(*i + 1, "*") && self.is_ident(*i + 2)) {
                name_at = star + 2;
            }
        }
        let unnamed = !self.is_ident(name_at)
            || name_at == lo
            || is_cpp_keyword(self.text(name_at))
            || matches!(
                self.text(name_at - 1),
                "::" | "const" | "volatile" | "unsigned" | "signed" | "struct" | "class"
            ) && !self.is_ident(name_at - 1);
        if unnamed {
            self.err(self.span(lo), "pre / post を書く関数の引数には名前が必要です");
            return None;
        }
        let rvalue =
            (lo..name_at).any(|i| self.is(i, "&") && self.is(i + 1, "&") && self.toks[i].hi == self.toks[i + 1].lo);
        let by_ref = (lo..name_at).any(|i| self.is(i, "&"));
        Some(ParamInfo {
            name: self.text(name_at).to_string(),
            text: self.slice(lo, end).to_string(),
            default: if end < hi { format!(" {}", self.slice(end, hi)) } else { String::new() },
            by_value: !by_ref,
            rvalue,
        })
    }

    /// The source between two positions with the inline edits made so far applied,
    /// which is how an attribute in the part of a declaration that gets repeated is converted.
    fn rendered(&self, lo: usize, hi: usize) -> String {
        let mut edits: Vec<&Edit> = self.edits.iter().filter(|e| !e.block && lo <= e.lo && e.hi <= hi).collect();
        edits.sort_by_key(|e| e.lo);
        let mut out = String::new();
        let mut pos = lo;
        for e in edits {
            out += &self.src[pos..e.lo];
            out += &e.text;
            pos = e.hi;
        }
        out += &self.src[pos..hi];
        out
    }

    fn function_with_contract(
        &mut self,
        clauses: &[Clause],
        start: usize,
        first: usize,
        after: usize,
        body_end: usize,
        sig: Signature,
    ) {
        let name_tok = self.toks[sig.name];
        let name = self.text(sig.name).to_string();
        let start_lo = self.toks[start].lo;
        let prefix = self.rendered(start_lo, name_tok.lo);
        let noexcept = self.slice(sig.tail_noexcept.0, sig.tail_noexcept.1);
        let trailing = sig.trailing.map_or("", |(lo, hi)| self.slice(lo, hi));
        let tail = format!(" {noexcept} {trailing}").trim_end().to_string();

        let mut with_defaults: Vec<String> = sig.params.iter().map(|p| format!("{}{}", p.text, p.default)).collect();
        with_defaults.push("std::source_location cc_caller = std::source_location::current()".to_string());
        let mut params: Vec<String> = sig.params.iter().map(|p| p.text.clone()).collect();
        params.push("[[maybe_unused]] std::source_location cc_caller".to_string());

        // The outer function, declared first so that the body can call itself through it.
        let mut forward = self.line_directive(start_lo);
        forward += &format!("{prefix}{name}({}){tail};", with_defaults.join(", "));
        self.block(start_lo, start_lo, forward);

        // The body stays where it is, under another name.
        self.inline(name_tok.lo, name_tok.hi, format!("cc_impl_{name}"));
        let clauses_hi = self.toks[after - 1].hi;
        let clauses_lo = self.toks[first].lo;
        let newlines = self.src[clauses_lo..clauses_hi].matches('\n').count();
        self.inline(clauses_lo, clauses_hi, "\n".repeat(newlines));

        let names: Vec<&str> = sig.params.iter().map(|p| p.name.as_str()).collect();
        let mut body = String::new();
        for c in clauses.iter().filter(|c| !c.is_post) {
            body += &self.line_directive(c.lo);
            body += &self.check_statement(c, &names, &mut Vec::new());
        }
        let mut olds = Vec::new();
        let mut post_checks = String::new();
        for c in clauses.iter().filter(|c| c.is_post) {
            post_checks += &self.line_directive(c.lo);
            post_checks += &self.check_statement(c, &names, &mut olds);
        }
        for (i, (src, cpp)) in olds.iter().enumerate() {
            body += &format!("const auto cc_old_{i} = ({cpp});  // {src}\n");
        }

        let used_after: HashSet<&str> =
            clauses.iter().filter(|c| c.is_post).flat_map(|c| (c.cond.0..c.cond.1).map(|i| self.text(i))).collect();
        let args: Vec<String> = sig
            .params
            .iter()
            .map(|p| {
                let n = &p.name;
                if p.rvalue || (p.by_value && !used_after.contains(n.as_str())) {
                    format!("static_cast<decltype({n})&&>({n})")
                } else {
                    n.clone()
                }
            })
            .collect();
        let call = format!("cc_impl_{name}({})", args.join(", "));
        let has_post = clauses.iter().any(|c| c.is_post);
        body += &self.line_directive(start_lo);
        if sig.returns_void {
            body += &format!("{call};\n{post_checks}");
        } else if has_post {
            body += &format!("decltype(auto) cc_r = {call};\n{post_checks}return cc_r;\n");
        } else {
            body += &format!("return {call};\n");
        }

        let mut wrapper = self.line_directive(start_lo);
        wrapper += &format!("{prefix}{name}({}){tail} {{\n{body}}}", params.join(", "));
        let end = self.toks[body_end].hi;
        self.block(end, end, wrapper);
    }

    /// One `if (!cond) report;` in the outer function.
    fn check_statement(&self, c: &Clause, params: &[&str], olds: &mut Vec<(String, String)>) -> String {
        let cond_src = one_line(self.slice(c.cond.0, c.cond.1));
        let label = c.label.clone().unwrap_or_else(|| cond_src.clone());
        let mut names: Vec<&str> = params.to_vec();
        if let Some(b) = &c.binder {
            names.push(b);
        }
        let base = olds.len();
        let cond = self.cooked(c.cond.0, c.cond.1, olds);
        let bind = c.binder.as_ref().map_or(String::new(), |b| format!("const auto& {b} = cc_r; "));
        if self.mode == Mode::Check {
            return format!("{{ {bind}(void)({cond}); }}\n");
        }
        let values = self.values(c.cond.0, c.cond.1, Some(&names), olds, base);
        let (blame, kind) = if c.is_post { ("callee", "post") } else { ("caller", "pre") };
        format!(
            "{{ {bind}if (!({cond})) ::cc::violation(::cc::Blame::{blame}, \"{kind}\", {}, {}, cc_caller, std::source_location::current(), {values}); }}\n",
            cpp_str(&label),
            cpp_str(&cond_src),
        )
    }

    /// The condition's text with each `old(x)` replaced by the copy made on entry.
    fn cooked(&self, lo: usize, hi: usize, olds: &mut Vec<(String, String)>) -> String {
        let mut out = String::new();
        let mut pos = if lo < hi { self.toks[lo].lo } else { return out };
        let mut j = lo;
        while j < hi {
            if self.is(j, "old") && self.is(j + 1, "(") {
                if let Some(close) = self.matching(j + 1) {
                    out += &self.src[pos..self.toks[j].lo];
                    let inner = self.slice(j + 2, close);
                    let n = olds.len();
                    olds.push((format!("old({inner})"), inner.to_string()));
                    out += &format!("cc_old_{n}");
                    pos = self.toks[close].hi;
                    j = close + 1;
                    continue;
                }
            }
            j += 1;
        }
        out += &self.src[pos..self.toks[hi - 1].hi];
        out
    }

    /// `{{"a.balance", ::cc::repr(a.balance)}, ...}` for the places a condition reads.
    /// With `only`, just chains that start with one of those names.
    fn values(&self, lo: usize, hi: usize, only: Option<&[&str]>, olds: &[(String, String)], base: usize) -> String {
        let mut seen: Vec<(String, String)> = Vec::new();
        let mut push = |name: String, cpp: String| {
            if !seen.iter().any(|(n, _)| *n == name) {
                seen.push((name, cpp));
            }
        };
        let mut old_index = base;
        let mut j = lo;
        while j < hi {
            if !self.is_ident(j) {
                j += 1;
                continue;
            }
            if self.is(j, "old") && self.is(j + 1, "(") {
                if let (Some(close), Some((src, _))) = (self.matching(j + 1), olds.get(old_index)) {
                    push(src.clone(), format!("cc_old_{old_index}"));
                    old_index += 1;
                    j = close + 1;
                    continue;
                }
            }
            if j > lo && matches!(self.text(j - 1), "." | "->" | "::") {
                j += 1;
                continue;
            }
            // Skip what is plainly not a value: `T<...>(`, `f(`, `T{`, `ns::`.
            if self.is(j + 1, "<") {
                if let Some(c) = self.matching_angle(j + 1).filter(|c| *c < hi) {
                    if matches!(self.text(c + 1), "(" | "{" | "::") {
                        j = c + 1;
                        continue;
                    }
                }
            }
            if matches!(self.text(j + 1), "(" | "{" | "::") || is_cpp_keyword(self.text(j)) {
                j += 1;
                continue;
            }
            if let Some(names) = only {
                if !names.contains(&self.text(j)) {
                    j += 1;
                    continue;
                }
            }
            let mut last = j;
            while last + 2 < hi && self.is(last + 1, ".") && self.is_ident(last + 2) {
                last += 2;
            }
            // `a.b.size()`: the object, not the method.
            if self.is(last + 1, "(") && last > j {
                last -= 2;
            }
            let chain = self.slice(j, last + 1).to_string();
            push(chain.clone(), chain);
            j = last + 1;
        }
        let items: Vec<String> =
            seen.iter().map(|(name, cpp)| format!("{{{}, ::cc::repr({cpp})}}", cpp_str(name))).collect();
        format!("{{{}}}", items.join(", "))
    }

    // ---------------------------------------------------- generated output

    /// Emits `type`, `schema` and enum names, now that all of them are known.
    fn declarations(&mut self) {
        let mut referenced: HashSet<String> = HashSet::new();
        for s in &self.schemas {
            for f in &s.fields {
                collect_named(&f.ty, &mut referenced);
            }
        }
        let used: Vec<usize> =
            (0..self.cpp_enums.len()).filter(|i| referenced.contains(&self.cpp_enums[*i].decl.name.text)).collect();
        for i in &used {
            if self.cpp_enums[*i].has_values {
                let d = &self.cpp_enums[*i].decl;
                let diag =
                    Diag::new(d.name.span, format!("スキーマで使う enum `{}` には値を指定できません", d.name.text))
                        .note("JSON との対応に値の順番を使います");
                self.diags.push(diag);
            }
        }
        let schemas = std::mem::take(&mut self.schemas);
        let types = std::mem::take(&mut self.types);
        let inline_enums = std::mem::take(&mut self.inline_enums);
        let cpp_enums = std::mem::take(&mut self.cpp_enums);
        let mut all_enums: Vec<EnumDecl> = inline_enums.iter().map(|(_, e)| e.clone()).collect();
        all_enums.extend(used.iter().map(|i| cpp_enums[*i].decl.clone()));
        let (ctx, diags) = check::check(&types, &schemas, &all_enums, self.src);
        self.diags.extend(diags);
        self.has_extensions = !types.is_empty() || !schemas.is_empty();
        let file = self.file.clone();
        let source = self.source;
        let mut em = Emitter::new(&ctx, source, &file);
        let mut blocks = Vec::new();
        for d in &types {
            blocks.push((d.span, em.type_decl(d)));
        }
        for d in &schemas {
            let mine: Vec<&EnumDecl> = inline_enums.iter().filter(|(at, _)| *at == d.span.lo).map(|(_, e)| e).collect();
            blocks.push((d.span, em.schema_decl(d, &mine)));
        }
        for i in &used {
            let e = &cpp_enums[*i];
            blocks.push((Span { lo: e.end, hi: e.end }, em.enum_names_decl(&e.decl)));
        }
        for (span, text) in blocks {
            self.block(span.lo, span.hi, text);
        }
    }

    fn assemble(mut self) -> Output {
        let uses_runtime = self.has_extensions || self.uses_contracts || self.strict || !self.tests.is_empty();
        let mut out = format!("// Generated by ccc from {}. Do not edit.\n", self.file);
        if self.strict {
            out += "#ifndef _GLIBCXX_ASSERTIONS\n#define _GLIBCXX_ASSERTIONS 1\n#endif\n";
        }
        if uses_runtime {
            out += "#include <cc/cc.hpp>\n";
        } else if self.uses_effects {
            out += "#include <cc/effects.hpp>\n";
        }
        out += &format!("#line 1 {}\n", cpp_str(&self.file));

        self.edits.sort_by_key(|e| (e.lo, e.hi));
        let mut pos = 0;
        for e in &self.edits {
            out += &self.src[pos..e.lo];
            if e.block {
                out += "\n";
                out += &e.text;
                if !e.text.ends_with('\n') {
                    out += "\n";
                }
                let (line, col) = self.source.line_col(e.hi);
                out += &format!("#line {line} {}\n", cpp_str(&self.file));
                out += &" ".repeat(col - 1);
            } else {
                out += &e.text;
            }
            pos = e.hi;
        }
        out += &self.src[pos..];

        if !self.tests.is_empty() {
            if !out.ends_with('\n') {
                out += "\n";
            }
            let cases: Vec<String> = self
                .tests
                .iter()
                .enumerate()
                .map(|(i, label)| format!("{{{}, cc_test_{i}}}", cpp_str(label)))
                .collect();
            out += "#ifdef CC_TEST\n";
            // A test build must not run the program's main, and must not change it either:
            // the tests run during initialization, and then the process exits.
            out += &format!(
                "[[maybe_unused]] static const int cc_test_runner = (std::exit(::cc::run_tests({{{}}})), 0);\n",
                cases.join(", ")
            );
            if !self.has_main {
                out += "int main() { return 0; }\n";
            }
            out += "#endif\n";
        }
        Output { cpp: out, has_tests: !self.tests.is_empty(), uses_effects: self.uses_effects, includes: self.includes }
    }
}

struct Signature {
    name: usize,
    params: Vec<ParamInfo>,
    tail_noexcept: (usize, usize),
    trailing: Option<(usize, usize)>,
    returns_void: bool,
}

fn directive_word(line: &str) -> &str {
    let rest = line[1..].trim_start();
    let end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
    &rest[..end]
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn collect_named(t: &Type, out: &mut HashSet<String>) {
    match &t.kind {
        TypeKind::Named(n) => {
            out.insert(n.clone());
        }
        TypeKind::Vec(i) | TypeKind::Raw(i) | TypeKind::Optional(i) => collect_named(i, out),
        _ => {}
    }
}
