//! The effect check. C++ hides calls in constructors, destructors, operators,
//! template instances and macros, so this does not read C++: it reads what Clang
//! resolved, from `-ast-dump=json`.
//!
//! That dump reaches a gigabyte once a heavy header is included, so it is read as a
//! stream and only the bodies of annotated functions are kept. Source positions in it
//! are written only when they change, which is why even skipped parts have to be scanned.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::process::{Command, Stdio};

use crate::effect_table;

/// A generated file and where its lines came from.
pub struct UserFile {
    /// The path Clang is given or finds through `#include`.
    pub path: String,
    /// The name of the source file, for messages.
    pub display: String,
    pub source: String,
    /// The C++ text, whose `#line` directives lead back to the source.
    pub cpp: String,
}

/// Runs the check on `files[0]`. Returns the rendered errors.
pub fn check(clang: &str, flags: &[String], files: &[UserFile]) -> Result<Vec<String>, String> {
    let mut child = Command::new(clang)
        .args(flags)
        .args(["-std=c++23", "-w", "-fsyntax-only", "-Xclang", "-ast-dump=json"])
        .arg(&files[0].path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{clang} を起動できません: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let users: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    // The dump nests deeply, and the reader recurses.
    let walker = std::thread::Builder::new()
        .stack_size(1 << 29)
        .spawn(move || {
            let mut w = Walker::new(stdout, users);
            w.document();
            w
        })
        .map_err(|e| e.to_string())?;
    let walked = walker.join().map_err(|_| "Clang の AST を読めませんでした".to_string())?;
    let status = child.wait().map_err(|e| e.to_string())?;
    let stderr = errors.join().unwrap_or_default();
    if !status.success() {
        return Err(format!("Clang が解析に失敗しました:\n{stderr}"));
    }
    Ok(walked.report(files))
}

// ------------------------------------------------------------------ reading

#[derive(Debug, Default, Clone)]
struct Loc {
    file: String,
    line: u32,
    col: u32,
}

#[derive(Debug)]
enum J {
    Str(String),
    Other,
    Obj(Vec<(String, J)>),
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(J::Str(s)) => Some(s),
            _ => None,
        }
    }

    fn truth(&self, key: &str) -> bool {
        matches!(self.get(key), Some(J::Other))
    }
}

#[derive(Debug, Clone)]
enum Tag {
    Effects(Vec<String>),
    Trusted(Vec<String>),
    Generated,
}

fn parse_list(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|e| !e.is_empty()).map(str::to_string).collect()
}

/// A thing in the body of an annotated function that has effects, or may.
#[derive(Debug)]
enum Use {
    /// A function named by a reference or member access.
    Fn {
        id: u64,
        name: String,
    },
    /// A member access that may name a field rather than a function.
    Member {
        id: u64,
        name: String,
    },
    Ctor {
        class: String,
        ctor_type: String,
    },
    Dtor {
        class: String,
    },
    DtorId {
        id: u64,
        name: String,
    },
    New,
    Delete,
    Throw,
    Indirect,
    Unresolved(String),
    Reinterpret,
    Asm,
    Coroutine,
}

#[derive(Debug)]
struct Located {
    what: Use,
    at: Loc,
    /// Inside `try { } catch (...)`, so `throw` does not get out.
    caught: bool,
}

struct FnInfo {
    qname: String,
    tag: Option<Tag>,
    prev: Option<u64>,
    is_virtual: bool,
    implicit: bool,
    record: Option<String>,
}

#[derive(Default)]
struct RecordInfo {
    seen: bool,
    generated: bool,
    /// Whether these special members are trivial or constexpr, in every instance seen.
    non_trivial_default: bool,
    non_trivial_copy: bool,
    non_trivial_move: bool,
    non_trivial_dtor: bool,
    non_trivial_copy_assign: bool,
    non_trivial_move_assign: bool,
}

struct Checked {
    name: String,
    declared: Vec<String>,
    uses: Vec<Located>,
}

struct Walker<R: Read> {
    rd: R,
    buf: Vec<u8>,
    pos: usize,
    len: usize,
    // Positions are written only when they change.
    file: String,
    line: u32,
    users: HashSet<String>,
    scope: Vec<String>,
    /// Inside a template pattern, whose bodies are not code that runs.
    pattern: bool,
    collecting: bool,
    uses: Vec<Located>,
    fns: HashMap<u64, FnInfo>,
    records: HashMap<String, RecordInfo>,
    annotated_ctors: HashMap<(String, String), Tag>,
    annotated_dtors: HashMap<String, Tag>,
    checked: Vec<Checked>,
}

#[derive(Clone, Copy, PartialEq)]
enum Parent {
    Other,
    ClassTemplate,
    FunctionTemplate,
    Lambda,
    Function,
}

#[derive(Default)]
struct Out {
    /// This expression names a function directly, so calling it is not an indirect call.
    direct: bool,
    tag: Option<Tag>,
    catch_all: bool,
    is_var: bool,
    is_catch: bool,
}

fn hex_id(s: &str) -> u64 {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)
}

fn is_function_kind(k: &str) -> bool {
    matches!(k, "FunctionDecl" | "CXXMethodDecl" | "CXXConstructorDecl" | "CXXDestructorDecl" | "CXXConversionDecl")
}

impl<R: Read> Walker<R> {
    fn new(rd: R, users: HashSet<String>) -> Self {
        Walker {
            rd,
            buf: vec![0; 1 << 20],
            pos: 0,
            len: 0,
            file: String::new(),
            line: 0,
            users,
            scope: Vec::new(),
            pattern: false,
            collecting: false,
            uses: Vec::new(),
            fns: HashMap::new(),
            records: HashMap::new(),
            annotated_ctors: HashMap::new(),
            annotated_dtors: HashMap::new(),
            checked: Vec::new(),
        }
    }

    #[inline]
    fn peek(&mut self) -> u8 {
        if self.pos == self.len {
            self.len = self.rd.read(&mut self.buf).unwrap_or(0);
            self.pos = 0;
            if self.len == 0 {
                return 0;
            }
        }
        self.buf[self.pos]
    }

    #[inline]
    fn bump(&mut self) -> u8 {
        let c = self.peek();
        self.pos += 1;
        c
    }

    #[inline]
    fn ws(&mut self) {
        while matches!(self.peek(), b' ' | b'\n' | b'\t' | b'\r') {
            self.pos += 1;
        }
    }

    fn at_end(&mut self) -> bool {
        self.ws();
        self.pos >= self.len && self.peek() == 0
    }

    fn read_str(&mut self) -> String {
        let mut out = Vec::new();
        self.ws();
        self.bump();
        loop {
            match self.bump() {
                b'"' | 0 => break,
                b'\\' => match self.bump() {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'u' => {
                        for _ in 0..4 {
                            self.bump();
                        }
                    }
                    c => out.push(c),
                },
                c => out.push(c),
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[inline]
    fn skip_str(&mut self) {
        self.bump();
        loop {
            match self.bump() {
                b'"' | 0 => return,
                b'\\' => {
                    self.bump();
                }
                _ => {}
            }
        }
    }

    fn read_number(&mut self) -> u32 {
        self.ws();
        let mut n = 0u32;
        while self.peek().is_ascii_digit() {
            n = n.wrapping_mul(10).wrapping_add((self.bump() - b'0') as u32);
        }
        n
    }

    /// A key, into a buffer that holds short ones. A longer key is no key this cares about.
    #[inline]
    fn read_key(&mut self, key: &mut [u8; 24]) -> usize {
        self.ws();
        self.bump();
        let mut n = 0;
        loop {
            match self.bump() {
                b'"' | 0 => break,
                b'\\' => {
                    self.bump();
                }
                c => {
                    if n < key.len() {
                        key[n] = c;
                    }
                    n += 1;
                }
            }
        }
        self.ws();
        self.bump(); // ':'
        n.min(key.len())
    }

    /// Skips a value, keeping up with the position it may have moved.
    fn skip(&mut self) {
        self.ws();
        match self.peek() {
            b'{' => {
                self.bump();
                let mut key = [0u8; 24];
                loop {
                    self.ws();
                    match self.peek() {
                        b'}' | 0 => {
                            self.bump();
                            return;
                        }
                        b',' => {
                            self.bump();
                            continue;
                        }
                        _ => {}
                    }
                    let n = self.read_key(&mut key);
                    match &key[..n] {
                        b"file" => self.file = self.read_str(),
                        b"line" => self.line = self.read_number(),
                        b"includedFrom" => {
                            let (f, l) = (std::mem::take(&mut self.file), self.line);
                            self.skip();
                            self.file = f;
                            self.line = l;
                        }
                        _ => self.skip(),
                    }
                }
            }
            b'[' => {
                self.bump();
                loop {
                    self.ws();
                    match self.peek() {
                        b']' | 0 => {
                            self.bump();
                            return;
                        }
                        b',' => {
                            self.bump();
                        }
                        _ => self.skip(),
                    }
                }
            }
            b'"' => self.skip_str(),
            _ => {
                while !matches!(self.peek(), b',' | b'}' | b']' | 0 | b' ' | b'\n') {
                    self.pos += 1;
                }
            }
        }
    }

    /// A small value in full. Used only for values without positions in them.
    fn value(&mut self) -> J {
        self.ws();
        match self.peek() {
            b'{' => {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.ws();
                    match self.peek() {
                        b'}' | 0 => {
                            self.bump();
                            return J::Obj(items);
                        }
                        b',' => {
                            self.bump();
                        }
                        _ => {
                            let k = self.read_str();
                            self.ws();
                            self.bump();
                            let v = self.value();
                            items.push((k, v));
                        }
                    }
                }
            }
            b'[' => {
                self.bump();
                loop {
                    self.ws();
                    match self.peek() {
                        b']' | 0 => {
                            self.bump();
                            return J::Other;
                        }
                        b',' => {
                            self.bump();
                        }
                        _ => {
                            self.value();
                        }
                    }
                }
            }
            b'"' => J::Str(self.read_str()),
            _ => {
                while !matches!(self.peek(), b',' | b'}' | b']' | 0 | b' ' | b'\n') {
                    self.pos += 1;
                }
                J::Other
            }
        }
    }

    /// A `loc` object, or the `begin` of a range. A macro expansion has two positions,
    /// and the one that counts for the user is where the macro was used.
    fn read_loc(&mut self) -> Loc {
        self.ws();
        self.bump(); // '{'
        let mut col = 0;
        let mut expansion = None;
        let mut key = [0u8; 24];
        loop {
            self.ws();
            match self.peek() {
                b'}' | 0 => {
                    self.bump();
                    break;
                }
                b',' => {
                    self.bump();
                    continue;
                }
                _ => {}
            }
            let n = self.read_key(&mut key);
            match &key[..n] {
                b"file" => self.file = self.read_str(),
                b"line" => self.line = self.read_number(),
                b"col" => col = self.read_number(),
                b"spellingLoc" => {
                    self.read_loc();
                }
                b"expansionLoc" => expansion = Some(self.read_loc()),
                b"includedFrom" => {
                    let (f, l) = (std::mem::take(&mut self.file), self.line);
                    self.skip();
                    self.file = f;
                    self.line = l;
                }
                _ => self.skip(),
            }
        }
        expansion.unwrap_or_else(|| Loc { file: self.file.clone(), line: self.line, col })
    }

    fn read_range(&mut self) -> Loc {
        self.ws();
        self.bump();
        let mut begin = Loc::default();
        let mut key = [0u8; 24];
        loop {
            self.ws();
            match self.peek() {
                b'}' | 0 => {
                    self.bump();
                    return begin;
                }
                b',' => {
                    self.bump();
                    continue;
                }
                _ => {}
            }
            let n = self.read_key(&mut key);
            if &key[..n] == b"begin" {
                begin = self.read_loc();
            } else {
                self.read_loc();
            }
        }
    }

    // ---------------------------------------------------------------- nodes

    fn document(&mut self) {
        if !self.at_end() {
            self.node(Parent::Other);
        }
    }

    fn push_use(&mut self, what: Use, at: &Loc) {
        self.uses.push(Located { what, at: at.clone(), caught: false });
    }

    fn node(&mut self, parent: Parent) -> Out {
        self.ws();
        self.bump(); // '{'
        let mut out = Out::default();
        let mut kind = String::new();
        let mut id = 0u64;
        let mut name = String::new();
        let mut ty: Option<J> = None;
        let mut prev = None;
        let mut is_virtual = false;
        let mut implicit = false;
        let mut constexpr = false;
        let mut cast = String::new();
        let mut value = String::new();
        let mut referenced: Option<J> = None;
        let mut member = None;
        let mut dtor: Option<J> = None;
        let mut ctor_type: Option<J> = None;
        let mut data: Option<J> = None;
        let mut at = Loc::default();
        // Whether what follows matters at all.
        let mut skip_all = false;
        let mut decided = false;
        let mut key = [0u8; 24];
        loop {
            self.ws();
            match self.peek() {
                b'}' | 0 => {
                    self.bump();
                    break;
                }
                b',' => {
                    self.bump();
                    continue;
                }
                _ => {}
            }
            let n = self.read_key(&mut key);
            if !decided && !kind.is_empty() {
                decided = true;
                skip_all = !self.interesting(&kind, parent);
            }
            if skip_all {
                match &key[..n] {
                    b"file" => self.file = self.read_str(),
                    b"line" => self.line = self.read_number(),
                    _ => self.skip(),
                }
                continue;
            }
            match &key[..n] {
                b"id" => id = hex_id(&self.read_str()),
                b"kind" => kind = self.read_str(),
                b"loc" => at = self.read_loc(),
                b"range" => {
                    let begin = self.read_range();
                    if begin.line != 0 && begin.col != 0 {
                        at = begin;
                    }
                }
                b"name" => name = self.read_str(),
                b"type" => ty = Some(self.value()),
                b"previousDecl" => prev = Some(hex_id(&self.read_str())),
                b"virtual" => {
                    self.skip();
                    is_virtual = true;
                }
                b"isImplicit" => {
                    self.skip();
                    implicit = true;
                }
                b"constexpr" => {
                    self.skip();
                    constexpr = true;
                }
                b"castKind" => cast = self.read_str(),
                b"value" => {
                    self.ws();
                    if self.peek() == b'"' {
                        value = self.read_str();
                    } else {
                        self.skip();
                    }
                }
                b"referencedDecl" => referenced = Some(self.value()),
                b"referencedMemberDecl" => member = Some(hex_id(&self.read_str())),
                b"dtor" if self.collecting => dtor = Some(self.value()),
                b"ctorType" => ctor_type = Some(self.value()),
                b"definitionData" => data = Some(self.value()),
                b"inner" => {
                    let extra = NodeExtra {
                        kind: &kind,
                        id,
                        name: &name,
                        ty: ty.as_ref(),
                        prev,
                        is_virtual,
                        implicit,
                        constexpr,
                        cast: &cast,
                        referenced: referenced.as_ref(),
                        member,
                        dtor: dtor.as_ref(),
                        ctor_type: ctor_type.as_ref(),
                        data: data.as_ref(),
                        at: &at,
                    };
                    self.inner(&extra, parent, &mut out);
                    // `inner` is last, and everything has been handled.
                    self.finish_object();
                    return out;
                }
                _ => self.skip(),
            }
        }
        // No children.
        if !skip_all && !kind.is_empty() {
            let extra = NodeExtra {
                kind: &kind,
                id,
                name: &name,
                ty: ty.as_ref(),
                prev,
                is_virtual,
                implicit,
                constexpr,
                cast: &cast,
                referenced: referenced.as_ref(),
                member,
                dtor: dtor.as_ref(),
                ctor_type: ctor_type.as_ref(),
                data: data.as_ref(),
                at: &at,
            };
            self.childless(&extra, parent, &mut out);
        }
        if kind == "StringLiteral" {
            out.tag = parse_tag(&value);
        }
        out.is_var = kind == "VarDecl";
        out.is_catch = kind == "CXXCatchStmt";
        out
    }

    fn finish_object(&mut self) {
        let mut key = [0u8; 24];
        loop {
            self.ws();
            match self.peek() {
                b'}' | 0 => {
                    self.bump();
                    return;
                }
                b',' => {
                    self.bump();
                }
                _ => {
                    self.read_key(&mut key);
                    self.skip();
                }
            }
        }
    }

    fn interesting(&self, kind: &str, parent: Parent) -> bool {
        if parent == Parent::Lambda && kind == "CXXRecordDecl" {
            return false;
        }
        if self.collecting {
            return kind != "ParmVarDecl" && kind != "ConstantExpr";
        }
        if parent == Parent::Function && !is_function_kind(kind) && !kind.ends_with("Attr") {
            return false;
        }
        is_container(kind) || is_function_kind(kind) || kind == "AnnotateAttr" || self.in_annotation(kind)
    }

    /// Inside an attribute everything leads to the string that holds the annotation.
    fn in_annotation(&self, kind: &str) -> bool {
        matches!(kind, "ConstantExpr" | "ImplicitCastExpr" | "StringLiteral")
    }

    fn scope_push(&mut self, name: &str) {
        self.scope.push(name.to_string());
    }

    fn qualified(&self, name: &str) -> String {
        let mut q = String::new();
        for s in &self.scope {
            if !s.is_empty() {
                q += s;
                q += "::";
            }
        }
        q + name
    }

    fn children(&mut self, parent: Parent, mut f: impl FnMut(&mut Self, Out)) {
        self.ws();
        self.bump(); // '['
        loop {
            self.ws();
            match self.peek() {
                b']' | 0 => {
                    self.bump();
                    return;
                }
                b',' => {
                    self.bump();
                }
                _ => {
                    let out = self.node(parent);
                    f(self, out);
                }
            }
        }
    }

    fn inner(&mut self, n: &NodeExtra, parent: Parent, out: &mut Out) {
        let kind = n.kind;
        if is_function_kind(kind) {
            return self.function(n);
        }
        if kind == "AnnotateAttr" {
            let mut tag = None;
            let saved = self.collecting;
            self.collecting = false;
            self.children(Parent::Other, |_, o| {
                if o.tag.is_some() {
                    tag = o.tag;
                }
            });
            self.collecting = saved;
            out.tag = tag;
            return;
        }
        if !self.collecting && !is_container(kind) {
            // ConstantExpr and casts on the way to an annotation's string.
            let mut tag = None;
            self.children(Parent::Other, |_, o| {
                if o.tag.is_some() {
                    tag = o.tag;
                }
            });
            out.tag = tag;
            return;
        }
        if is_container(kind) {
            return self.container(n, parent);
        }
        self.expression(n, out);
    }

    fn container(&mut self, n: &NodeExtra, parent: Parent) {
        let kind = n.kind;
        let is_record = matches!(
            kind,
            "CXXRecordDecl" | "ClassTemplateSpecializationDecl" | "ClassTemplatePartialSpecializationDecl"
        );
        let names_scope = is_record || kind == "NamespaceDecl";
        let was_pattern = self.pattern;
        let was_collecting = self.collecting;
        // A local class is not part of the function it is in.
        self.collecting = false;
        if kind == "ClassTemplatePartialSpecializationDecl"
            || (kind == "CXXRecordDecl" && parent == Parent::ClassTemplate)
        {
            self.pattern = true;
        }
        if names_scope {
            self.scope_push(n.name);
        }
        if is_record && !n.implicit {
            if let Some(d) = n.data {
                let q = normalize(&self.qualified(""));
                let q = q.trim_end_matches("::").to_string();
                let r = self.records.entry(q).or_default();
                r.seen = true;
                note_flags(r, d);
            }
        }
        let child_parent = match kind {
            "ClassTemplateDecl" => Parent::ClassTemplate,
            "FunctionTemplateDecl" => Parent::FunctionTemplate,
            _ => Parent::Other,
        };
        let mut generated = false;
        let qualified_here = self.qualified("");
        let is_function_template = kind == "FunctionTemplateDecl";
        let mut first_function_done = false;
        self.ws();
        self.bump(); // '['
        loop {
            self.ws();
            match self.peek() {
                b']' | 0 => {
                    self.bump();
                    break;
                }
                b',' => {
                    self.bump();
                }
                _ => {
                    // The first function under a function template is its pattern.
                    let saved = self.pattern;
                    if is_function_template && !first_function_done {
                        self.pattern = true;
                    }
                    let before = self.fns.len();
                    let o = self.node(child_parent);
                    if is_function_template && self.fns.len() != before {
                        first_function_done = true;
                    }
                    self.pattern = saved;
                    if matches!(o.tag, Some(Tag::Generated)) {
                        generated = true;
                    }
                }
            }
        }
        if generated {
            let q = normalize(qualified_here.trim_end_matches("::"));
            self.records.entry(q).or_default().generated = true;
        }
        if names_scope {
            self.scope.pop();
        }
        self.pattern = was_pattern;
        self.collecting = was_collecting;
    }

    fn function(&mut self, n: &NodeExtra) {
        let user = !self.pattern && self.users.contains(&n.at.file);
        let saved_uses = std::mem::take(&mut self.uses);
        let saved_collecting = self.collecting;
        self.collecting = false;
        let mut tag = None;
        self.ws();
        self.bump(); // '['
        loop {
            self.ws();
            match self.peek() {
                b']' | 0 => {
                    self.bump();
                    break;
                }
                b',' => {
                    self.bump();
                }
                _ => {
                    self.collecting = user;
                    let o = self.node(Parent::Function);
                    self.collecting = false;
                    if o.tag.is_some() {
                        tag = o.tag;
                    }
                }
            }
        }
        let uses = std::mem::replace(&mut self.uses, saved_uses);
        self.collecting = saved_collecting;
        self.record_function(n, tag, Some(uses), user);
    }

    fn record_function(&mut self, n: &NodeExtra, tag: Option<Tag>, uses: Option<Vec<Located>>, user: bool) {
        let qname = self.qualified(n.name);
        let record = self.scope.last().map(|_| {
            let q = self.qualified("");
            normalize(q.trim_end_matches("::"))
        });
        let shape = n.ty.and_then(|t| t.str("qualType")).map(strip_noexcept).unwrap_or_default();
        if let (Some(t), Some(r)) = (&tag, &record) {
            match n.kind {
                "CXXConstructorDecl" => {
                    self.annotated_ctors.insert((r.clone(), shape), t.clone());
                }
                "CXXDestructorDecl" => {
                    self.annotated_dtors.insert(r.clone(), t.clone());
                }
                _ => {}
            }
        }
        if let (Some(Tag::Effects(declared)), Some(uses), true) = (&tag, uses, user) {
            self.checked.push(Checked { name: display_name(&qname), declared: declared.clone(), uses });
        }
        self.fns
            .insert(n.id, FnInfo { qname, tag, prev: n.prev, is_virtual: n.is_virtual, implicit: n.implicit, record });
    }

    fn childless(&mut self, n: &NodeExtra, parent: Parent, out: &mut Out) {
        if is_function_kind(n.kind) {
            self.record_function(n, None, None, false);
            return;
        }
        if n.kind == "CXXRecordDecl" && !n.implicit {
            if let Some(d) = n.data {
                self.scope_push(n.name);
                let q = normalize(self.qualified("").trim_end_matches("::"));
                self.scope.pop();
                let r = self.records.entry(q).or_default();
                r.seen = true;
                note_flags(r, d);
            }
        }
        if self.collecting && parent != Parent::Lambda {
            self.expression_effects(n, out);
        }
    }

    /// A node in the body of a function that may need checking.
    fn expression(&mut self, n: &NodeExtra, out: &mut Out) {
        self.expression_effects(n, out);
        let kind = n.kind;
        let is_call = matches!(kind, "CallExpr" | "CXXMemberCallExpr" | "CXXOperatorCallExpr" | "UserDefinedLiteral");
        let passes_direct = matches!(kind, "ImplicitCastExpr" | "ParenExpr" | "UnaryOperator");
        let child_parent = if kind == "LambdaExpr" { Parent::Lambda } else { Parent::Other };
        let try_begin = self.uses.len();
        let mut try_end = self.uses.len();
        let mut first = true;
        let mut direct_child = false;
        let mut catch_all = false;
        let mut has_var = false;
        let at = n.at.clone();
        let is_try = kind == "CXXTryStmt";
        let mut children = |w: &mut Self, o: Out| {
            if first {
                first = false;
                direct_child = o.direct;
                if is_try {
                    try_end = w.uses.len();
                }
            }
            catch_all |= o.catch_all;
            has_var |= o.is_var;
        };
        if constexpr_skip(n) {
            self.skip();
        } else {
            self.children(child_parent, &mut children);
        }
        if is_call && !first && !direct_child {
            self.push_use(Use::Indirect, &at);
        }
        if passes_direct && direct_child {
            out.direct = true;
        }
        if is_try && catch_all {
            for u in &mut self.uses[try_begin..try_end] {
                u.caught = true;
            }
        }
        if kind == "CXXCatchStmt" && !has_var {
            out.catch_all = true;
        }
        out.is_var = kind == "VarDecl";
    }

    fn expression_effects(&mut self, n: &NodeExtra, out: &mut Out) {
        let at = n.at;
        match n.kind {
            "DeclRefExpr" => {
                if let Some(r) = n.referenced {
                    let kind = r.str("kind").unwrap_or("");
                    if is_function_kind(kind) {
                        out.direct = true;
                        let (id, name) = (hex_id(r.str("id").unwrap_or("")), r.str("name").unwrap_or("").to_string());
                        self.push_use(Use::Fn { id, name }, at);
                    }
                }
            }
            "MemberExpr" => {
                out.direct = true;
                if let Some(id) = n.member {
                    self.push_use(Use::Member { id, name: n.name.to_string() }, at);
                }
            }
            "CXXConstructExpr" | "CXXTemporaryObjectExpr" => {
                if let (Some(t), Some(c)) = (n.ty, n.ctor_type) {
                    let class = class_of(t);
                    let ctor_type = c.str("qualType").map(strip_noexcept).unwrap_or_default();
                    if let Some(class) = class {
                        self.push_use(Use::Ctor { class, ctor_type }, at);
                    }
                }
            }
            "CXXBindTemporaryExpr" => {
                if let Some(d) = n.dtor {
                    let (id, name) = (hex_id(d.str("id").unwrap_or("")), d.str("name").unwrap_or("").to_string());
                    self.push_use(Use::DtorId { id, name }, at);
                }
            }
            "VarDecl" => {
                if let Some(class) = n.ty.and_then(class_of).filter(|_| !n.constexpr) {
                    self.push_use(Use::Dtor { class }, at);
                }
            }
            "CXXNewExpr" => self.push_use(Use::New, at),
            "CXXDeleteExpr" => self.push_use(Use::Delete, at),
            "CXXThrowExpr" => self.push_use(Use::Throw, at),
            "CXXReinterpretCastExpr" => self.push_use(Use::Reinterpret, at),
            "CStyleCastExpr" if matches!(n.cast, "BitCast" | "IntegralToPointer" | "PointerToIntegral") => {
                self.push_use(Use::Reinterpret, at);
            }
            "GCCAsmStmt" | "MSAsmStmt" => self.push_use(Use::Asm, at),
            "CoawaitExpr" | "CoyieldExpr" | "CoreturnStmt" => self.push_use(Use::Coroutine, at),
            "UnresolvedLookupExpr" | "UnresolvedMemberExpr" | "CXXDependentScopeMemberExpr" => {
                out.direct = true;
                self.push_use(Use::Unresolved(n.name.to_string()), at);
            }
            "CXXPseudoDestructorExpr" => out.direct = true,
            _ => {}
        }
    }

    // ------------------------------------------------------------- judging

    fn tag_of(&self, id: u64) -> Option<&Tag> {
        let mut cur = id;
        for _ in 0..64 {
            let info = self.fns.get(&cur)?;
            if info.tag.is_some() {
                return info.tag.as_ref();
            }
            cur = info.prev?;
        }
        None
    }

    /// Out-of-line definitions are at the wrong place to know their class: their first declaration does.
    fn name_of(&self, id: u64) -> Option<String> {
        let mut info = self.fns.get(&id)?;
        for _ in 0..64 {
            match info.prev.and_then(|p| self.fns.get(&p)) {
                Some(p) => info = p,
                None => break,
            }
        }
        Some(normalize(&info.qname))
    }

    /// What a use needs, how to describe it, and an explanation if it is `unsafe` by default.
    fn judge(&self, u: &Use) -> Option<(Vec<String>, String, Option<String>)> {
        let unsafe_name = |shown: &str, what: String| {
            let note = format!("`{shown}` に効果の宣言がないため unsafe 扱いです。[[cc::effects(...)]] を付けるか、効果の一覧に載っている関数を使ってください");
            Some((vec!["unsafe".to_string()], what, Some(note)))
        };
        match u {
            Use::Fn { id, name } | Use::Member { id, name } => {
                let member = matches!(u, Use::Member { .. });
                let Some(q) = self.name_of(*id) else {
                    // The call operator of a lambda: its body is checked as part of this function.
                    if name == "operator()" || member {
                        return None;
                    }
                    return unsafe_name(name, format!("`{name}` の呼び出し"));
                };
                let shown = display_name(&q);
                let what = format!("`{shown}` の呼び出し");
                let info = &self.fns[id];
                let virtual_call = member && (info.is_virtual || self.prev_is_virtual(*id));
                if virtual_call {
                    let note = "仮想関数は呼び先が実行時に決まるため unsafe 扱いです".to_string();
                    return Some((vec!["unsafe".to_string()], what, Some(note)));
                }
                if let Some(Tag::Effects(list) | Tag::Trusted(list)) = self.tag_of(*id) {
                    return Some((list.clone(), what, None));
                }
                if let Some(list) = effect_table::lookup(&q) {
                    return Some((list, what, None));
                }
                if info.implicit {
                    if let Some(r) = info.record.as_ref().and_then(|r| self.records.get(r)) {
                        let non_trivial = if name.starts_with("operator=") {
                            r.non_trivial_copy_assign && r.non_trivial_move_assign
                        } else {
                            true
                        };
                        if !non_trivial || r.generated {
                            return None;
                        }
                    }
                }
                unsafe_name(&shown, what)
            }
            Use::Ctor { class, ctor_type } => {
                let simple = class.rsplit("::").next().unwrap_or(class);
                let q = format!("{class}::{simple}");
                let what = format!("コンストラクタ `{q}`");
                if let Some(Tag::Effects(list) | Tag::Trusted(list)) =
                    self.annotated_ctors.get(&(class.clone(), ctor_type.clone()))
                {
                    return Some((list.clone(), what, None));
                }
                if let Some(list) = effect_table::lookup(&q) {
                    return Some((list, what, None));
                }
                let r = self.records.get(class);
                let Some(r) = r.filter(|r| r.seen) else {
                    return unsafe_name(&q, what);
                };
                let shape = ctor_shape(ctor_type);
                let trivial = match shape {
                    Shape::Default => !r.non_trivial_default,
                    Shape::Copy => !r.non_trivial_copy,
                    Shape::Move => !r.non_trivial_move,
                    Shape::Other => false,
                };
                if trivial {
                    return None;
                }
                if r.generated {
                    let list = if shape == Shape::Copy { vec!["alloc".to_string()] } else { vec![] };
                    return Some((list, what, None));
                }
                unsafe_name(&q, what)
            }
            Use::Dtor { class } => self.judge_dtor(class),
            Use::DtorId { id, name } => {
                let class = self
                    .fns
                    .get(id)
                    .and_then(|i| i.record.clone())
                    .unwrap_or_else(|| name.trim_start_matches('~').to_string());
                self.judge_dtor(&class)
            }
            Use::New => Some((vec!["alloc".to_string()], "`new`".to_string(), None)),
            Use::Delete => Some((vec!["alloc".to_string()], "`delete`".to_string(), None)),
            Use::Throw => Some((vec!["throw".to_string()], "`throw` 式".to_string(), None)),
            Use::Indirect => Some((
                vec!["unsafe".to_string()],
                "呼び先の分からない呼び出し".to_string(),
                Some("関数ポインタなどは呼び先が実行時に決まるため unsafe 扱いです".to_string()),
            )),
            Use::Unresolved(n) => Some((
                vec!["unsafe".to_string()],
                format!("解決できない呼び出し `{n}`"),
                Some("呼び先が型に依存して決まらない呼び出しは unsafe 扱いです".to_string()),
            )),
            Use::Reinterpret => Some((vec!["unsafe".to_string()], "ポインタの再解釈".to_string(), None)),
            Use::Asm => Some((vec!["unsafe".to_string()], "インラインアセンブリ".to_string(), None)),
            Use::Coroutine => Some((vec!["async".to_string()], "コルーチンの中断".to_string(), None)),
        }
    }

    fn prev_is_virtual(&self, id: u64) -> bool {
        let mut cur = self.fns.get(&id).and_then(|i| i.prev);
        for _ in 0..64 {
            let Some(info) = cur.and_then(|c| self.fns.get(&c)) else { return false };
            if info.is_virtual {
                return true;
            }
            cur = info.prev;
        }
        false
    }

    fn judge_dtor(&self, class: &str) -> Option<(Vec<String>, String, Option<String>)> {
        let simple = class.rsplit("::").next().unwrap_or(class);
        let q = format!("{class}::~{simple}");
        let what = format!("デストラクタ `{q}`（スコープの終わりなどで暗黙に呼ばれます）");
        if let Some(Tag::Effects(list) | Tag::Trusted(list)) = self.annotated_dtors.get(class) {
            return Some((list.clone(), what, None));
        }
        if let Some(list) = effect_table::lookup(&q) {
            return Some((list, what, None));
        }
        let Some(r) = self.records.get(class).filter(|r| r.seen) else {
            // An enum or a scalar behind an alias: nothing to destroy.
            return None;
        };
        if !r.non_trivial_dtor || r.generated {
            return None;
        }
        let note = format!("`{q}` に効果の宣言がないため unsafe 扱いです。[[cc::effects(...)]] を付けてください");
        Some((vec!["unsafe".to_string()], what, Some(note)))
    }

    fn report(&self, files: &[UserFile]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut found: Vec<(String, u32, u32, String)> = Vec::new();
        for f in &self.checked {
            let declared: HashSet<&str> = f.declared.iter().map(String::as_str).collect();
            let allows = |e: &str| declared.contains(e) || (e == "alloc" && declared.contains("io"));
            for u in &f.uses {
                let Some((needs, what, note)) = self.judge(&u.what) else { continue };
                let missing: Vec<&str> = needs
                    .iter()
                    .map(String::as_str)
                    .filter(|e| !allows(e))
                    .filter(|e| !(u.caught && *e == "throw"))
                    .collect();
                if missing.is_empty() {
                    continue;
                }
                let file = files.iter().find(|x| x.path == u.at.file);
                let Some(file) = file else { continue };
                let line = source_line(&file.cpp, u.at.line);
                let col = u.at.col;
                let message = format!(
                    "{what}には効果 {} が必要ですが、`{}` は effects({}) しか宣言していません",
                    missing.join(", "),
                    f.name,
                    f.declared.join(", ")
                );
                if !seen.insert((file.path.clone(), line, message.clone())) {
                    continue;
                }
                let text = file.source.lines().nth(line as usize - 1).unwrap_or("");
                let gutter = line.to_string().len();
                let col_ok = (col as usize) <= text.chars().count().max(1);
                let col_shown = if col_ok { col.max(1) } else { 1 };
                let mut r = format!("{}:{line}:{col_shown}: エラー: {message}\n", file.display);
                r += &format!("{:gutter$} |\n{line} | {text}\n", "");
                r += &format!("{:gutter$} | {}^\n", "", " ".repeat(col_shown as usize - 1));
                if let Some(n) = note {
                    r += &format!("{:gutter$} = 補足: {n}\n", "");
                }
                found.push((file.display.clone(), line, col_shown, r));
            }
        }
        found.sort();
        found.into_iter().map(|f| f.3).collect()
    }
}

struct NodeExtra<'a> {
    kind: &'a str,
    id: u64,
    name: &'a str,
    ty: Option<&'a J>,
    prev: Option<u64>,
    is_virtual: bool,
    implicit: bool,
    constexpr: bool,
    cast: &'a str,
    referenced: Option<&'a J>,
    member: Option<u64>,
    dtor: Option<&'a J>,
    ctor_type: Option<&'a J>,
    data: Option<&'a J>,
    at: &'a Loc,
}

/// A `constexpr` variable's initializer runs at compile time.
fn constexpr_skip(n: &NodeExtra) -> bool {
    n.kind == "VarDecl" && n.constexpr
}

fn is_container(kind: &str) -> bool {
    matches!(
        kind,
        "TranslationUnitDecl"
            | "NamespaceDecl"
            | "LinkageSpecDecl"
            | "CXXRecordDecl"
            | "ClassTemplateSpecializationDecl"
            | "ClassTemplatePartialSpecializationDecl"
            | "ClassTemplateDecl"
            | "FunctionTemplateDecl"
            | "FriendDecl"
            | "ExportDecl"
    )
}

fn parse_tag(value: &str) -> Option<Tag> {
    // The dump holds the C++ spelling of the literal.
    let s = value.strip_prefix('"')?.strip_suffix('"')?;
    if let Some(list) = s.strip_prefix("effects:") {
        return Some(Tag::Effects(parse_list(list)));
    }
    if let Some(list) = s.strip_prefix("trusted:") {
        return Some(Tag::Trusted(parse_list(list)));
    }
    (s == "generated").then_some(Tag::Generated)
}

fn note_flags(r: &mut RecordInfo, d: &J) {
    let non_trivial = |key: &str| d.get(key).is_some_and(|m| m.truth("nonTrivial"));
    let constexpr = d.get("defaultCtor").is_some_and(|m| m.truth("isConstexpr") || m.truth("defaultedIsConstexpr"));
    r.non_trivial_default |= non_trivial("defaultCtor") && !constexpr;
    r.non_trivial_copy |= non_trivial("copyCtor");
    r.non_trivial_move |= non_trivial("moveCtor");
    r.non_trivial_dtor |= non_trivial("dtor");
    r.non_trivial_copy_assign |= non_trivial("copyAssign");
    r.non_trivial_move_assign |= non_trivial("moveAssign");
}

fn strip_noexcept(s: &str) -> String {
    s.replace(" noexcept", "")
}

#[derive(PartialEq, Clone, Copy)]
enum Shape {
    Default,
    Copy,
    Move,
    Other,
}

fn ctor_shape(ty: &str) -> Shape {
    let Some(args) = ty.strip_prefix("void (").and_then(|s| s.strip_suffix(')')) else { return Shape::Other };
    if args.is_empty() {
        Shape::Default
    } else if args.contains(',') {
        Shape::Other
    } else if args.ends_with("&&") {
        Shape::Move
    } else if args.starts_with("const ") && args.ends_with('&') {
        Shape::Copy
    } else {
        Shape::Other
    }
}

/// The class a type names, when a value of it has a class to construct or destroy.
fn class_of(t: &J) -> Option<String> {
    let ty = t.str("desugaredQualType").or_else(|| t.str("qualType"))?;
    let mut s = ty.trim();
    for prefix in ["const ", "volatile ", "struct ", "class "] {
        s = s.strip_prefix(prefix).unwrap_or(s);
    }
    if s.ends_with('*') || s.ends_with('&') || s.starts_with('(') || s.contains("(*)") || s.contains("(&)") {
        return None;
    }
    let s = s.split('[').next().unwrap_or(s).trim();
    let s = s.trim_end_matches(" const");
    let plain = strip_template_args(s);
    let first = plain.chars().next()?;
    (first.is_alphabetic() || first == '_' || first == ':').then(|| normalize(&plain))
}

fn strip_template_args(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Without inline namespaces, which are not part of how anyone names these.
fn normalize(q: &str) -> String {
    let q = q.trim_start_matches("::");
    let parts: Vec<&str> = q
        .split("::")
        .filter(|p| !matches!(*p, "__cxx11" | "__1" | "_V2") && !p.starts_with("json_abi_") && !p.is_empty())
        .collect();
    parts.join("::")
}

fn display_name(q: &str) -> String {
    let n = normalize(q);
    match n.rsplit_once("::") {
        Some((scope, last)) => format!("{scope}::{}", last.trim_start_matches("cc_impl_")),
        None => n.trim_start_matches("cc_impl_").to_string(),
    }
}

/// The source line of a line in generated C++, from its `#line` directives.
fn source_line(cpp: &str, actual: u32) -> u32 {
    let mut current = 1u32;
    for (i, text) in cpp.lines().enumerate() {
        let number = i as u32 + 1;
        if number == actual {
            return current;
        }
        match text.strip_prefix("#line ").and_then(|r| r.split_whitespace().next()).and_then(|n| n.parse().ok()) {
            Some(n) => current = n,
            None => current += 1,
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_come_back_through_line_directives() {
        let cpp = "// head\n#line 1 \"a\"\nx\ny\n#line 7 \"a\"\nz\n";
        assert_eq!(source_line(cpp, 3), 1);
        assert_eq!(source_line(cpp, 4), 2);
        assert_eq!(source_line(cpp, 6), 7);
    }

    #[test]
    fn names_lose_what_nobody_writes() {
        assert_eq!(normalize("std::__cxx11::basic_string::size"), "std::basic_string::size");
        assert_eq!(display_name("ns::cc_impl_withdraw"), "ns::withdraw");
    }

    #[test]
    fn classes_of_types() {
        let ty = |s: &str| J::Obj(vec![("qualType".to_string(), J::Str(s.to_string()))]);
        assert_eq!(class_of(&ty("std::vector<int, std::allocator<int>>")).as_deref(), Some("std::vector"));
        assert_eq!(class_of(&ty("const Foo")).as_deref(), Some("Foo"));
        assert_eq!(class_of(&ty("Foo &")), None);
        assert_eq!(class_of(&ty("Foo *")), None);
        assert_eq!(class_of(&ty("(lambda at a.cpp:1:2)")), None);
    }

    #[test]
    fn positions_carry_over_through_skipped_values() {
        let text = br#"{"a":{"b":[{"loc":{"file":"x.cpp","line":3,"col":1}}]},"c":{"line":9},"d":{"includedFrom":{"file":"no"}}}"#;
        let mut w = Walker::new(&text[..], HashSet::new());
        w.skip();
        assert_eq!(w.file, "x.cpp");
        assert_eq!(w.line, 9);
    }
}
