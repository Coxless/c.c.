//! C++23 code generation.
//!
//! Output order: types / schemas / cpp blocks in source order, then
//! prototypes of every function, then function bodies, then `main`.
//! Each statement is preceded by `#line` so that C++ compiler errors and
//! `std::source_location` (used by contract reports) point into the .c.c file.

use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::*;
use crate::check::{builtin, integral_literal, Ctx};
use crate::diag::Source;

const CPP_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "break",
    "case",
    "catch",
    "char",
    "char8_t",
    "char16_t",
    "char32_t",
    "class",
    "compl",
    "concept",
    "const",
    "consteval",
    "constexpr",
    "constinit",
    "const_cast",
    "continue",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
    "main",
];

/// C.C. names that collide with C++ keywords get a trailing underscore.
pub fn mangle(name: &str) -> String {
    if CPP_KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// A C++ string literal with the given content.
pub fn cpp_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\t' => out += "\\t",
            '\r' => out += "\\r",
            '\0' => out += "\\0",
            _ => out.push(c),
        }
    }
    out + "\""
}

pub fn cpp_type(t: &Type) -> String {
    match &t.kind {
        TypeKind::Prim(p) => match p {
            Prim::I32 => "std::int32_t",
            Prim::I64 => "std::int64_t",
            Prim::U8 => "std::uint8_t",
            Prim::U16 => "std::uint16_t",
            Prim::U32 => "std::uint32_t",
            Prim::U64 => "std::uint64_t",
            Prim::F64 => "double",
            Prim::Bool => "bool",
            Prim::String => "std::string",
        }
        .into(),
        TypeKind::Json => "::cc::Json".into(),
        TypeKind::Named(n) => mangle(n),
        TypeKind::Vec(t) => format!("std::vector<{}>", cpp_type(t)),
        TypeKind::Raw(t) => format!("::cc::Raw<{}>", cpp_type(t)),
        TypeKind::Optional(t) => format!("std::optional<{}>", cpp_type(t)),
        TypeKind::Result(t, e) => format!("std::expected<{}, ::cc::{}>", cpp_type(t), cpp_type(e)),
    }
}

pub fn emit(prog: &Program, ctx: &Ctx, src: &Source) -> String {
    let file = src.name.rsplit('/').next().unwrap_or(src.name).to_string();
    let mut e =
        Emitter { ctx, src, file, out: String::new(), indent: 0, tmp: 0, olds: HashMap::new(), qualify_fns: false };
    e.program(prog);
    e.out
}

struct Emitter<'a> {
    ctx: &'a Ctx<'a>,
    src: &'a Source<'a>,
    file: String,
    out: String,
    indent: usize,
    tmp: usize,
    /// `old(...)` expressions of the current function, by span start.
    olds: HashMap<usize, usize>,
    /// Emitting outside `namespace cc_user`, so calls to this file's functions must name it.
    qualify_fns: bool,
}

/// How a schema field is stored and decoded.
struct FieldPlan {
    /// Type held in the struct (inside `std::optional` for `T?`).
    storage: String,
    /// Type decoded from JSON before refinement.
    decoded: String,
    /// Whether `decoded` still has to go through `storage::try_from`.
    refine: bool,
    /// Nested refined class to generate for a `where` / `matches` on the field.
    class: Option<String>,
}

const PREC_OR: u8 = 1;
const PREC_AND: u8 = 2;
const PREC_CMP: u8 = 3;
const PREC_ADD: u8 = 4;
const PREC_MUL: u8 = 5;
const PREC_UNARY: u8 = 6;
const PREC_POSTFIX: u8 = 7;
const PREC_ATOM: u8 = 8;

impl<'a> Emitter<'a> {
    fn line(&mut self, span: Span) {
        let _ = writeln!(self.out, "#line {} {}", self.src.line(span.lo), cpp_str(&self.file));
    }

    fn w(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out += "  ";
        }
        self.out += s;
        self.out += "\n";
    }

    fn text(&self, span: Span) -> &'a str {
        self.src.slice(span)
    }

    fn program(&mut self, prog: &Program) {
        let src_name = self.file.clone();
        self.out += &format!("// Generated by ccc from {src_name}. Do not edit.\n#include <cc/cc.hpp>\n");
        // Conditions such as `self <= 65535` on a u16 are tautological for the type, which is fine.
        self.out += "#pragma GCC diagnostic ignored \"-Wtype-limits\"\n";
        // Pragmas rather than compiler flags, so that a plain `g++` on the output enforces them too.
        self.out += "CC_STRICT\n";
        for item in &prog.items {
            match item {
                Item::Type(d) => {
                    self.out += "\n";
                    self.line(d.span);
                    let name = mangle(&d.name.text);
                    self.refined_class(&name, &d.name.text, &d.base, d.cond.as_ref(), d.matches.as_ref());
                }
                Item::Schema(d) => {
                    self.out += "\n";
                    self.schema(d);
                }
                Item::Cpp { code, span } => {
                    self.out += "\n";
                    self.raw_cpp(code, *span);
                }
                Item::Enum(d) => {
                    self.out += "\n";
                    self.enum_decl(d);
                }
                Item::Fn(_) | Item::Test(_) => {}
            }
        }
        let fns: Vec<&FnDecl> = prog
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Fn(d) if d.body.is_some() => Some(d),
                _ => None,
            })
            .collect();
        // Functions live in a namespace of their own. At global scope `fn abs(a: i64)` would
        // overload the C library's abs(int), and `abs(-1)` would call that one, contract unchecked.
        if !fns.is_empty() {
            self.out += "\nnamespace cc_user {\n";
            for d in &fns {
                self.line(d.span);
                let sig = self.signature(d, true);
                self.w(&format!("{sig};"));
            }
            self.out += "}  // namespace cc_user\n";
        }
        // After the prototypes, so an invariant can call a function of this file.
        self.qualify_fns = true;
        for item in &prog.items {
            if let Item::Schema(d) = item {
                self.invariant_bodies(d);
            }
        }
        self.qualify_fns = false;
        if !fns.is_empty() {
            self.out += "\nnamespace cc_user {\n";
            for (i, d) in fns.iter().enumerate() {
                if i > 0 {
                    self.out += "\n";
                }
                self.function(d);
            }
            self.out += "}  // namespace cc_user\n";
        }
        let main = fns.iter().find(|d| Self::is_main(d)).map(|d| {
            if d.params.is_empty() {
                "int main() { return ::cc::run_main(cc_user::cc_main); }\n"
            } else {
                "int main(int argc, char** argv) {\n  return ::cc::run_main([&] { return cc_user::cc_main(::cc::command_args(argc, argv)); });\n}\n"
            }
        });
        let tests: Vec<&TestDecl> = prog
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Test(t) => Some(t),
                _ => None,
            })
            .collect();
        if tests.is_empty() {
            if let Some(main) = main {
                self.out += "\n";
                self.out += main;
            }
            return;
        }
        self.out += "\n#ifdef CC_TEST\nnamespace cc_user {\n";
        for (i, t) in tests.iter().enumerate() {
            self.test(i, t);
            self.out += "\n";
        }
        self.out += "}  // namespace cc_user\n";
        let cases: Vec<String> =
            tests.iter().enumerate().map(|(i, t)| format!("{{{}, cc_user::cc_test_{i}}}", cpp_str(&t.label))).collect();
        self.out += &format!("int main() {{ return ::cc::run_tests({{{}}}); }}\n", cases.join(", "));
        if let Some(main) = main {
            self.out += "#else\n";
            self.out += main;
        }
        self.out += "#endif\n";
    }

    /// A `cpp { }` block. It is the user's own C++, so the C.C. strictness does not apply.
    fn raw_cpp(&mut self, code: &str, span: Span) {
        self.out += "CC_CPP_BEGIN\n";
        self.line(span);
        self.out += code.trim_end();
        self.out += "\nCC_CPP_END\n";
    }

    fn test(&mut self, i: usize, t: &TestDecl) {
        self.olds.clear();
        self.tmp = 0;
        self.line(t.func.span);
        self.w(&format!("static void cc_test_{i}() {{"));
        self.block(t.func.body.as_ref().unwrap(), &t.func);
        self.w("}");
    }

    fn enum_decl(&mut self, d: &EnumDecl) {
        let name = mangle(&d.name.text);
        let variants: Vec<String> = d.variants.iter().map(|v| mangle(&v.text)).collect();
        let names: Vec<String> = d.variants.iter().map(|v| cpp_str(&v.text)).collect();
        self.line(d.span);
        self.w(&format!("enum class {name} {{ {} }};", variants.join(", ")));
        self.w(&format!(
            "inline std::vector<std::string_view> cc_enum_names({name}) {{ return {{{}}}; }}",
            names.join(", ")
        ));
    }

    fn invariant_bodies(&mut self, d: &SchemaDecl) {
        let name = mangle(&d.name.text);
        for (i, inv) in d.invariants.iter().enumerate() {
            let cond = self.expr(&inv.cond);
            self.out += "\n";
            self.line(inv.span);
            self.w(&format!("inline bool {name}::cc_invariant_{i}() const {{ return {cond}; }}"));
        }
    }

    // ------------------------------------------------------- refined types

    fn refined_class(&mut self, cls: &str, display: &str, base: &Type, cond: Option<&Expr>, matches: Option<&Name>) {
        let b = cpp_type(base);
        let integral = matches!(base.kind, TypeKind::Prim(p) if p.is_integral());
        let cx = if matches!(base.kind, TypeKind::Prim(p) if p != Prim::String) && matches.is_none() {
            "constexpr "
        } else {
            ""
        };
        let mut checks = Vec::new();
        let mut srcs = Vec::new();
        if let Some(c) = cond {
            checks.push(self.expr_prec(c, PREC_CMP));
            srcs.push(self.text(c.span).to_string());
        }
        if let Some(m) = matches {
            checks.push(format!("{}(self)", self.fn_name(&m.text)));
            srcs.push(format!("matches {}", m.text));
        }
        let check = match (cond, checks.len()) {
            (Some(c), 1) => self.expr(c),
            _ => checks.join(" && "),
        };
        let src = srcs.join(" && ");

        self.w(&format!("class {cls} {{"));
        self.w(" public:");
        self.indent += 1;
        self.w(&format!("using underlying_type = {b};"));
        self.w(&format!("static {cx}bool check(const {b}& self) {{ return {check}; }}"));
        self.w(&format!("static {cx}std::expected<{cls}, ::cc::RefineError> try_from({b} v) {{"));
        self.w(&format!(
            "  if (!check(v)) return std::unexpected(::cc::RefineError{{{}, {}}});",
            cpp_str(display),
            cpp_str(&src)
        ));
        self.w(&format!("  return {cls}(std::move(v));"));
        self.w("}");
        if integral {
            // Any integer, range-checked first: a parameter of type `b` would wrap it before `check`.
            self.w("template <::cc::integer_like A>");
            self.w(&format!("  requires(!std::same_as<A, {b}>)"));
            self.w(&format!("static {cx}std::expected<{cls}, ::cc::RefineError> try_from(const A& a) {{"));
            self.w(&format!(
                "  if (!::cc::fits<{b}>(a)) return std::unexpected(::cc::RefineError{{{}, {}}});",
                cpp_str(display),
                cpp_str(&format!("{} の範囲内", self.text(base.span)))
            ));
            self.w(&format!("  return try_from(static_cast<{b}>(::cc::integer_value(a)));"));
            self.w("}");
            // Anything else that converts to `b` (a double, say) must not reach the overload above.
            self.w("template <class A>");
            self.w(&format!("  requires(!std::same_as<A, {b}> && !::cc::integer_like<A>)"));
            self.w("static void try_from(const A&) = delete;");
        }
        if integral {
            self.w("// Literals: checked at compile time, no runtime check left.");
            self.w(&format!("template <{b} V>"));
            self.w("  requires(check(V))");
            self.w(&format!("static consteval {cls} of() {{ return {cls}(V); }}"));
        }
        self.w(&format!("{cx}const {b}& get() const {{ return v_; }}"));
        self.w(&format!("{cx}operator const {b}&() const {{ return v_; }}"));
        self.w(&format!("friend {cx}bool operator==(const {cls}&, const {cls}&) = default;"));
        if matches!(base.kind, TypeKind::Prim(Prim::String)) {
            // std::string's operator== is a template, so the conversion above does not reach it.
            self.w(&format!("friend bool operator==(const {cls}& a, const {b}& b) {{ return a.v_ == b; }}"));
        }
        if matches!(base.kind, TypeKind::Vec(_)) {
            self.w("auto begin() const { return v_.begin(); }");
            self.w("auto end() const { return v_.end(); }");
        }
        self.indent -= 1;
        self.w(" private:");
        self.indent += 1;
        self.w(&format!("explicit {cx}{cls}({b} v) : v_(std::move(v)) {{}}"));
        self.w(&format!("{b} v_;"));
        self.indent -= 1;
        self.w("};");
    }

    // -------------------------------------------------------------- schemas

    fn field_plan(&self, f: &Field) -> FieldPlan {
        let inner = inner_type(&f.ty);
        if f.cond.is_some() || f.matches.is_some() {
            let cls = format!("{}_t", f.name.text);
            return FieldPlan { storage: cls.clone(), decoded: cpp_type(inner), refine: true, class: Some(cls) };
        }
        if let (Some(_), Some(d)) = (&f.transform, self.ctx.refined(inner)) {
            return FieldPlan { storage: cpp_type(inner), decoded: cpp_type(&d.base), refine: true, class: None };
        }
        FieldPlan { storage: cpp_type(inner), decoded: cpp_type(inner), refine: false, class: None }
    }

    /// C++ type of the plain value behind a field (what a default value must be).
    fn field_value_type(&self, f: &Field) -> String {
        let inner = inner_type(&f.ty);
        match self.ctx.refined(inner) {
            Some(d) if f.cond.is_none() && f.matches.is_none() => cpp_type(&d.base),
            _ => cpp_type(inner),
        }
    }

    fn schema(&mut self, d: &SchemaDecl) {
        let name = mangle(&d.name.text);
        self.line(d.span);
        self.w(&format!("struct {name} {{"));
        self.indent += 1;
        let plans: Vec<FieldPlan> = d.fields.iter().map(|f| self.field_plan(f)).collect();
        for (f, p) in d.fields.iter().zip(&plans) {
            if let Some(cls) = &p.class {
                self.line(f.span);
                let display = format!("{}.{}", d.name.text, f.name.text);
                self.refined_class(cls, &display, inner_type(&f.ty), f.cond.as_ref(), f.matches.as_ref());
                self.out += "\n";
            }
        }
        for (f, p) in d.fields.iter().zip(&plans) {
            let ty = if is_optional(&f.ty) { format!("std::optional<{}>", p.storage) } else { p.storage.clone() };
            self.w(&format!("{ty} {};", mangle(&f.name.text)));
        }
        if !d.invariants.is_empty() {
            self.out += "\n";
        }
        for i in 0..d.invariants.len() {
            self.w(&format!("bool cc_invariant_{i}() const;"));
        }

        self.out += "\n";
        self.w(&format!(
            "static std::optional<{name}> parse_at(const ::cc::Json& j, const std::string& path, std::vector<::cc::FieldError>& errs) {{"
        ));
        self.indent += 1;
        self.w("if (!::cc::expect_object(j, path, errs)) return std::nullopt;");
        self.w("const auto cc_n = errs.size();");
        for (f, p) in d.fields.iter().zip(&plans) {
            self.parse_field(f, p);
        }
        if !d.open {
            let known: Vec<String> = d.fields.iter().map(|f| cpp_str(&f.name.text)).collect();
            self.w(&format!("::cc::reject_unknown(j, path, {{{}}}, errs);", known.join(", ")));
        }
        self.w("if (errs.size() != cc_n) return std::nullopt;");
        let members: Vec<String> = d
            .fields
            .iter()
            .map(|f| {
                if is_optional(&f.ty) {
                    format!("std::move(f_{})", f.name.text)
                } else {
                    format!("std::move(*f_{})", f.name.text)
                }
            })
            .collect();
        self.w(&format!("{name} out{{{}}};", members.join(", ")));
        for (i, inv) in d.invariants.iter().enumerate() {
            let src = self.text(inv.cond.span);
            let label = inv.label.as_deref().unwrap_or(src);
            self.w(&format!(
                "if (!out.cc_invariant_{i}()) errs.push_back(::cc::invariant_failure(path, {}, {}));",
                cpp_str(label),
                cpp_str(src)
            ));
        }
        if !d.invariants.is_empty() {
            self.w("if (errs.size() != cc_n) return std::nullopt;");
        }
        self.w("return out;");
        self.indent -= 1;
        self.w("}");

        self.w(&format!("static std::expected<{name}, ::cc::ParseError> parse(const ::cc::Json& j) {{"));
        self.w("  std::vector<::cc::FieldError> errs;");
        self.w("  auto v = parse_at(j, \"\", errs);");
        self.w(&format!(
            "  if (!v) return std::unexpected(::cc::ParseError{{{}, std::move(errs)}});",
            cpp_str(&d.name.text)
        ));
        self.w("  return std::move(*v);");
        self.w("}");
        self.w(&format!(
            "static std::expected<{name}, ::cc::ParseError> parse(const ::cc::Raw<::cc::Json>& raw) {{ return parse(raw.unchecked()); }}"
        ));

        self.w("::cc::Json to_json() const {");
        self.w("  ::cc::Json j = ::cc::Json::object();");
        for f in &d.fields {
            self.w(&format!("  j[{}] = ::cc::encode({});", cpp_str(&f.name.text), mangle(&f.name.text)));
        }
        self.w("  return j;");
        self.w("}");
        self.indent -= 1;
        self.w("};");
    }

    fn parse_field(&mut self, f: &Field, p: &FieldPlan) {
        let key = cpp_str(&f.name.text);
        let var = format!("f_{}", f.name.text);
        self.line(f.span);
        self.w(&format!("std::optional<{}> {var};", p.storage));
        self.w(&format!("if (const auto* v = ::cc::field(j, {key})) {{"));
        self.indent += 1;
        let path = format!("::cc::join(path, {key})");
        if p.refine || f.transform.is_some() {
            self.w(&format!("if (auto d = ::cc::decode<{}>(*v, {path}, errs)) {{", p.decoded));
            if let Some(t) = &f.transform {
                self.w(&format!("  *d = {}(std::move(*d));", self.fn_name(&t.text)));
            }
            if p.refine {
                self.w(&format!("  {var} = ::cc::refine_into<{}>(std::move(*d), {path}, errs);", p.storage));
            } else {
                self.w(&format!("  {var} = std::move(*d);"));
            }
            self.w("}");
        } else {
            self.w(&format!("{var} = ::cc::decode<{}>(*v, {path}, errs);", p.storage));
        }
        self.indent -= 1;
        match &f.default {
            Some(def) => {
                let value = match &def.kind {
                    ExprKind::List(items) => {
                        let items: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
                        format!("{}{{{}}}", self.field_value_type(f), items.join(", "))
                    }
                    _ => self.coerce(def, inner_type(&f.ty)),
                };
                self.w("} else {");
                self.w(&format!("  {var} = ::cc::refine_into<{}>({value}, {path}, errs);", p.storage));
                self.w("}");
            }
            None if is_optional(&f.ty) => self.w("}"),
            None => {
                self.w("} else {");
                self.w(&format!("  ::cc::missing({path}, errs);"));
                self.w("}");
            }
        }
    }

    // ------------------------------------------------------------ functions

    fn is_main(d: &FnDecl) -> bool {
        d.name.text == "main"
    }

    fn signature(&self, d: &FnDecl, with_default: bool) -> String {
        let ret =
            if Self::is_main(d) { "int".to_string() } else { d.ret.as_ref().map_or("void".to_string(), cpp_type) };
        let name = if Self::is_main(d) { "cc_main".to_string() } else { mangle(&d.name.text) };
        let mut params: Vec<String> = d
            .params
            .iter()
            .map(|p| format!("{}{} {}", cpp_type(&p.ty), if p.by_ref { "&" } else { "" }, mangle(&p.name.text)))
            .collect();
        if d.is_contract {
            params.push(format!(
                "std::source_location cc_caller{}",
                if with_default { " = std::source_location::current()" } else { "" }
            ));
        }
        let noexcept = match &d.effects {
            Some(list) if !list.iter().any(|e| e.text == "throw") => " noexcept",
            _ => "",
        };
        format!("{ret} {name}({}){noexcept}", params.join(", "))
    }

    fn function(&mut self, d: &FnDecl) {
        self.olds.clear();
        self.tmp = 0;
        self.line(d.span);
        let sig = self.signature(d, false);
        self.w(&format!("{sig} {{"));
        self.indent += 1;

        for c in &d.pres {
            let src = self.text(c.cond.span);
            let label = c.label.clone().unwrap_or_else(|| src.to_string());
            let cond = self.expr(&c.cond);
            let values = self.values(&c.cond);
            self.line(c.span);
            self.w(&format!(
                "if (!({cond})) ::cc::violation(::cc::Blame::caller, \"pre\", {}, {}, cc_caller, std::source_location::current(), {values});",
                cpp_str(&label),
                cpp_str(src)
            ));
        }

        let mut olds = Vec::new();
        for c in &d.posts {
            collect_olds(&c.cond, &mut olds);
        }
        for (i, o) in olds.iter().enumerate() {
            self.olds.insert(o.span.lo, i);
            let ExprKind::Old(inner) = &o.kind else { unreachable!() };
            let value = self.expr(inner);
            self.w(&format!("const auto cc_old_{i} = {value};  // old({})", self.text(inner.span)));
        }

        let has_post = !d.posts.is_empty();
        if has_post {
            match &d.ret {
                Some(t) => self.w(&format!("auto cc_post = [&]([[maybe_unused]] const {}& cc_result) {{", cpp_type(t))),
                None => self.w("auto cc_post = [&]() {"),
            }
            self.indent += 1;
            for c in &d.posts {
                let src = self.text(c.cond.span);
                let label = c.label.clone().unwrap_or_else(|| src.to_string());
                let cond = self.expr(&c.cond);
                let binder = c.binder.as_ref().map(|b| b.text.as_str());
                let values = self.values(&c.cond);
                let bind = binder.map_or(String::new(), |b| format!("const auto& {} = cc_result; ", mangle(b)));
                self.line(c.span);
                self.w(&format!(
                    "if ({bind}!({cond})) ::cc::violation(::cc::Blame::callee, \"post\", {}, {}, cc_caller, std::source_location::current(), {values});",
                    cpp_str(&label),
                    cpp_str(src)
                ));
            }
            self.indent -= 1;
            self.w("};");
        }

        let body = d.body.as_ref().unwrap();
        for s in &body.stmts {
            self.stmt(s, d);
        }
        if has_post && d.ret.is_none() {
            self.w("cc_post();");
        }
        if Self::is_main(d) && d.ret.is_none() {
            self.w("return 0;");
        }
        self.indent -= 1;
        if d.ret.is_some() {
            // The closing brace is where C++ reports a missing `return`: name the function's line.
            self.line(d.span);
        }
        self.w("}");
    }

    /// `{{"a.balance", ::cc::repr(a.balance)}, ...}` for the places a condition reads.
    fn values(&self, cond: &Expr) -> String {
        let mut places: Vec<(String, String)> = Vec::new();
        self.places(cond, &mut places);
        let items: Vec<String> =
            places.iter().map(|(name, cpp)| format!("{{{}, ::cc::repr({cpp})}}", cpp_str(name))).collect();
        format!("{{{}}}", items.join(", "))
    }

    fn places(&self, e: &Expr, out: &mut Vec<(String, String)>) {
        let mut push = |name: String, cpp: String| {
            if !out.iter().any(|(n, _)| *n == name) {
                out.push((name, cpp));
            }
        };
        match &e.kind {
            ExprKind::Ident(_) => push(self.text(e.span).to_string(), self.expr(e)),
            ExprKind::Field(..) if is_place(e) => push(self.text(e.span).to_string(), self.expr(e)),
            ExprKind::Old(_) => push(self.text(e.span).to_string(), self.expr(e)),
            ExprKind::Field(obj, _) => self.places(obj, out),
            ExprKind::Index(a, i) => {
                self.places(a, out);
                self.places(i, out);
            }
            ExprKind::Call(callee, args) => {
                if let ExprKind::Field(obj, _) = &callee.kind {
                    self.places(obj, out);
                }
                args.iter().for_each(|a| self.places(a, out));
            }
            ExprKind::Unary(_, a) => self.places(a, out),
            ExprKind::Binary(_, a, b) => {
                self.places(a, out);
                self.places(b, out);
            }
            ExprKind::Compare(first, rest) => {
                self.places(first, out);
                rest.iter().for_each(|(_, r)| self.places(r, out));
            }
            ExprKind::List(items) => items.iter().for_each(|i| self.places(i, out)),
            _ => {}
        }
    }

    // ----------------------------------------------------------- statements

    /// Emits the check for a trailing `?` and returns the expression for its value.
    fn value(&mut self, e: &Expr, target: Option<&Type>) -> String {
        if let ExprKind::Try(inner) = &e.kind {
            let t = format!("cc_t{}", self.tmp);
            self.tmp += 1;
            let v = self.expr(inner);
            self.w(&format!("auto {t} = {v};"));
            self.w(&format!("if (!{t}) throw ::cc::Failure(::cc::describe({t}.error()));"));
            return format!("std::move(*{t})");
        }
        match target {
            Some(t) => self.coerce(e, t),
            None => self.expr(e),
        }
    }

    fn block(&mut self, b: &Block, d: &FnDecl) {
        self.indent += 1;
        for s in &b.stmts {
            self.stmt(s, d);
        }
        self.indent -= 1;
    }

    fn stmt(&mut self, s: &Stmt, d: &FnDecl) {
        if let StmtKind::Cpp(code) = &s.kind {
            return self.raw_cpp(code, s.span);
        }
        self.line(s.span);
        match &s.kind {
            StmtKind::Let { name, ty, init } => {
                let v = self.value(init, ty.as_ref());
                let t = match ty {
                    Some(t) => cpp_type(t),
                    // `auto` would make `let sum = 0;` a 32-bit int.
                    None if int_constant(init) => "std::int64_t".to_string(),
                    None => "auto".to_string(),
                };
                self.w(&format!("{t} {} = {v};", mangle(name)));
            }
            StmtKind::Assign { target, op, value } => {
                let op = match op {
                    AssignOp::Set => "=",
                    AssignOp::Add => "+=",
                    AssignOp::Sub => "-=",
                    AssignOp::Mul => "*=",
                    AssignOp::Div => "/=",
                    AssignOp::Rem => "%=",
                };
                let (t, v) = (self.expr(target), self.expr(value));
                self.w(&format!("{t} {op} {v};"));
            }
            StmtKind::Expr(e) => {
                if matches!(e.kind, ExprKind::Try(_)) {
                    self.value(e, None);
                } else {
                    let v = self.expr(e);
                    self.w(&format!("{v};"));
                }
            }
            StmtKind::Return(value) => {
                let has_post = !d.posts.is_empty();
                match (value, &d.ret) {
                    (Some(v), Some(t)) => {
                        let v = self.value(v, Some(t));
                        if has_post {
                            self.w(&format!("{{ {} cc_r = {v}; cc_post(cc_r); return cc_r; }}", cpp_type(t)));
                        } else {
                            self.w(&format!("return {v};"));
                        }
                    }
                    (Some(v), None) => {
                        let v = self.value(v, None);
                        self.w(&format!("return {v};"));
                    }
                    (None, _) if Self::is_main(d) => self.w("return 0;"),
                    (None, _) if has_post => self.w("{ cc_post(); return; }"),
                    (None, _) => self.w("return;"),
                }
            }
            StmtKind::If { cond, then, els } => {
                let c = self.expr(cond);
                self.w(&format!("if ({c}) {{"));
                self.block(then, d);
                if let Some(b) = els {
                    self.w("} else {");
                    self.block(b, d);
                }
                self.w("}");
            }
            StmtKind::For { var, iter, body } => {
                let var = mangle(var);
                match iter {
                    ForIter::Range(a, b) => {
                        let (a, b) = (self.expr(a), self.expr(b));
                        self.w(&format!("for (std::int64_t {var} = {a}; {var} < {b}; ++{var}) {{"));
                    }
                    ForIter::Each(e) => {
                        let e = self.expr(e);
                        self.w(&format!("for (auto&& {var} : {e}) {{"));
                    }
                }
                self.block(body, d);
                self.w("}");
            }
            StmtKind::While { cond, body } => {
                let c = self.expr(cond);
                self.w(&format!("while ({c}) {{"));
                self.block(body, d);
                self.w("}");
            }
            StmtKind::Block(b) => {
                self.w("{");
                self.block(b, d);
                self.w("}");
            }
            StmtKind::Cpp(_) => unreachable!("handled above"),
            StmtKind::Match { subject, arms } => {
                let m = format!("cc_m{}", self.tmp);
                self.tmp += 1;
                let v = self.expr(subject);
                self.w(&format!("auto {m} = {v};"));
                let arm = |kinds: [ArmKind; 2]| arms.iter().find(|a| kinds.contains(&a.kind));
                self.w(&format!("if ({m}) {{"));
                if let Some(a) = arm([ArmKind::Ok, ArmKind::Some]) {
                    self.arm(a, &format!("std::move(*{m})"), d);
                }
                self.w("} else {");
                if let Some(a) = arm([ArmKind::Err, ArmKind::None]) {
                    self.arm(a, &format!("std::move({m}.error())"), d);
                }
                self.w("}");
            }
            StmtKind::Try { body, var, handler } => {
                self.w("try {");
                self.block(body, d);
                self.w("} catch (const std::exception& cc_e) {");
                self.w(&format!("  [[maybe_unused]] std::string {} = cc_e.what();", mangle(&var.text)));
                self.block(handler, d);
                self.w("}");
            }
            StmtKind::With { entries, body, .. } => {
                let n = self.tmp;
                self.tmp += 1;
                let files: Vec<String> =
                    entries.iter().map(|(k, v)| format!("{{{}, {}}}", self.expr(k), self.expr(v))).collect();
                self.w("{");
                self.w(&format!("  ::cc::MemoryFs cc_h{n}{{{}}};", files.join(", ")));
                self.w(&format!("  ::cc::WithIo cc_w{n}{{cc_h{n}}};"));
                self.block(body, d);
                self.w("}");
            }
            StmtKind::Expect(cond) => {
                let src = self.text(cond.span);
                let c = self.expr(cond);
                let values = self.values(cond);
                self.w(&format!(
                    "if (!({c})) ::cc::expect_failed({}, std::source_location::current(), {values});",
                    cpp_str(src)
                ));
            }
        }
    }

    fn arm(&mut self, a: &MatchArm, value: &str, d: &FnDecl) {
        if let Some(b) = &a.binder {
            self.w(&format!("  [[maybe_unused]] auto {} = {value};", mangle(&b.text)));
        }
        self.block(&a.body, d);
    }

    // ---------------------------------------------------------- expressions

    fn fn_name(&self, name: &str) -> String {
        if builtin(name).is_some() {
            format!("::cc::{name}")
        } else if self.qualify_fns && self.ctx.fns.get(name).is_some_and(|d| d.body.is_some()) {
            format!("::cc_user::{}", mangle(name))
        } else {
            mangle(name)
        }
    }

    /// `e` used where `target` is expected: refined literals become `T::of<V>()`.
    fn coerce(&self, e: &Expr, target: &Type) -> String {
        if integral_literal(e) && self.ctx.literal_refined(target).is_some() {
            return format!("{}::of<{}>()", cpp_type(target), self.expr(e));
        }
        if let ExprKind::List(items) = &e.kind {
            let items: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
            return format!("{}{{{}}}", cpp_type(target), items.join(", "));
        }
        self.expr(e)
    }

    fn expr(&self, e: &Expr) -> String {
        self.expr_prec(e, 0)
    }

    fn expr_prec(&self, e: &Expr, min: u8) -> String {
        let (s, prec) = match &e.kind {
            ExprKind::Int(t) => {
                let big = t.parse::<u64>().is_ok_and(|v| v > i64::MAX as u64);
                (if big { format!("{t}ULL") } else { t.clone() }, PREC_ATOM)
            }
            ExprKind::Float(t) => (t.clone(), PREC_ATOM),
            ExprKind::Str { lit } => (format!("std::string({lit})"), PREC_ATOM),
            ExprKind::Bool(b) => (b.to_string(), PREC_ATOM),
            ExprKind::None => ("std::nullopt".into(), PREC_ATOM),
            ExprKind::List(items) => {
                let elem = if !items.is_empty() && items.iter().all(int_constant) { "<std::int64_t>" } else { "" };
                let items: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
                (format!("std::vector{elem}{{{}}}", items.join(", ")), PREC_ATOM)
            }
            ExprKind::Ident(n) => match self.ctx.variants.get(&e.span.lo) {
                Some(cpp) => (cpp.clone(), PREC_ATOM),
                None => (mangle(n), PREC_ATOM),
            },
            ExprKind::SelfValue => ("self".into(), PREC_ATOM),
            ExprKind::Path(t, m) => (format!("{}::{}", mangle(t), mangle(m)), PREC_ATOM),
            ExprKind::Field(obj, f) => (format!("{}.{}", self.expr_prec(obj, PREC_POSTFIX), mangle(f)), PREC_POSTFIX),
            ExprKind::Index(a, i) => (format!("::cc::at({}, {})", self.expr(a), self.expr(i)), PREC_POSTFIX),
            ExprKind::Call(callee, args) => {
                let (callee_s, params) = match &callee.kind {
                    ExprKind::Ident(n) => (self.fn_name(n), self.ctx.fns.get(n.as_str()).map(|d| &d.params)),
                    _ => (self.expr_prec(callee, PREC_POSTFIX), None),
                };
                let args: Vec<String> = match params {
                    Some(ps) if ps.len() == args.len() => {
                        args.iter().zip(ps).map(|(a, p)| self.coerce(a, &p.ty)).collect()
                    }
                    _ => args.iter().map(|a| self.expr(a)).collect(),
                };
                (format!("{callee_s}({})", args.join(", ")), PREC_POSTFIX)
            }
            ExprKind::Unary(op, a) => {
                let op = if *op == UnOp::Neg { "-" } else { "!" };
                (format!("{op}{}", self.expr_prec(a, PREC_UNARY)), PREC_UNARY)
            }
            ExprKind::Binary(op, a, b) => {
                let (sym, p) = match op {
                    BinOp::Or => ("||", PREC_OR),
                    BinOp::And => ("&&", PREC_AND),
                    BinOp::Add => ("+", PREC_ADD),
                    BinOp::Sub => ("-", PREC_ADD),
                    BinOp::Mul => ("*", PREC_MUL),
                    BinOp::Div => ("/", PREC_MUL),
                    BinOp::Rem => ("%", PREC_MUL),
                };
                (format!("{} {sym} {}", self.expr_prec(a, p), self.expr_prec(b, p + 1)), p)
            }
            ExprKind::Compare(first, rest) => {
                let mut parts = Vec::new();
                let mut lhs = first.as_ref();
                for (op, rhs) in rest {
                    let sym = match op {
                        CmpOp::Eq => "==",
                        CmpOp::Ne => "!=",
                        CmpOp::Lt => "<",
                        CmpOp::Le => "<=",
                        CmpOp::Gt => ">",
                        CmpOp::Ge => ">=",
                    };
                    parts.push(format!("{} {sym} {}", self.expr_prec(lhs, PREC_ADD), self.expr_prec(rhs, PREC_ADD)));
                    lhs = rhs;
                }
                if parts.len() == 1 {
                    (parts.pop().unwrap(), PREC_CMP)
                } else {
                    (parts.join(" && "), PREC_AND)
                }
            }
            ExprKind::Old(_) => (format!("cc_old_{}", self.olds[&e.span.lo]), PREC_ATOM),
            // Rejected by the checker outside statement position.
            ExprKind::Try(_) => unreachable!("`?` outside statement position"),
        };
        if prec < min {
            format!("({s})")
        } else {
            s
        }
    }
}

/// An integer expression made of literals only, which C++ would type as `int`.
/// A literal too large for i64 is left to C++, which gives it an unsigned type.
fn int_constant(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(t) => t.parse::<i64>().is_ok(),
        ExprKind::Unary(UnOp::Neg, a) => int_constant(a),
        ExprKind::Binary(op, a, b) => !matches!(op, BinOp::And | BinOp::Or) && int_constant(a) && int_constant(b),
        _ => false,
    }
}

fn inner_type(t: &Type) -> &Type {
    match &t.kind {
        TypeKind::Optional(i) => i,
        _ => t,
    }
}

fn is_optional(t: &Type) -> bool {
    matches!(t.kind, TypeKind::Optional(_))
}

/// `a`, `a.b`, `a.b.c`: something whose value is worth showing in a report.
fn is_place(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Ident(_) => true,
        ExprKind::Field(obj, _) => is_place(obj),
        _ => false,
    }
}

fn collect_olds<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match &e.kind {
        ExprKind::Old(_) => out.push(e),
        ExprKind::Field(a, _) | ExprKind::Unary(_, a) | ExprKind::Try(a) => collect_olds(a, out),
        ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) => {
            collect_olds(a, out);
            collect_olds(b, out);
        }
        ExprKind::Compare(first, rest) => {
            collect_olds(first, out);
            rest.iter().for_each(|(_, r)| collect_olds(r, out));
        }
        ExprKind::Call(callee, args) => {
            collect_olds(callee, out);
            args.iter().for_each(|a| collect_olds(a, out));
        }
        ExprKind::List(items) => items.iter().for_each(|i| collect_olds(i, out)),
        _ => {}
    }
}
