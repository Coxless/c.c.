//! C++23 for `type`, `schema` and the enums schemas use. Each declaration is
//! emitted where it was written, so ordinary C++ scoping applies to it.

use std::fmt::Write;

use crate::ast::*;
use crate::check::{is_builtin, Ctx};

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
];

/// Names that collide with C++ keywords get a trailing underscore.
pub fn mangle(name: &str) -> String {
    if CPP_KEYWORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

pub fn is_cpp_keyword(name: &str) -> bool {
    CPP_KEYWORDS.contains(&name)
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
    }
}

fn fn_name(name: &str) -> String {
    if is_builtin(name) {
        format!("::cc::{name}")
    } else {
        mangle(name)
    }
}

/// Generated code is not the user's to fix, and conditions such as `self <= 65535`
/// on a u16 are tautological for the type, so neither warnings nor strict mode apply.
const QUIET_BEGIN: &str = "#pragma GCC diagnostic push
#pragma GCC diagnostic ignored \"-Wtype-limits\"
#pragma GCC diagnostic ignored \"-Wconversion\"
#pragma GCC diagnostic ignored \"-Wsign-conversion\"
#pragma GCC diagnostic ignored \"-Wsign-compare\"
";
const QUIET_END: &str = "#pragma GCC diagnostic pop\n";

pub struct Emitter<'a> {
    pub ctx: &'a Ctx<'a>,
    pub src: &'a crate::diag::Source<'a>,
    pub file: &'a str,
    out: String,
    indent: usize,
}

const PREC_OR: u8 = 1;
const PREC_AND: u8 = 2;
const PREC_CMP: u8 = 3;
const PREC_ADD: u8 = 4;
const PREC_MUL: u8 = 5;
const PREC_UNARY: u8 = 6;
const PREC_POSTFIX: u8 = 7;
const PREC_ATOM: u8 = 8;

/// How a schema field is stored and validated.
struct FieldPlan {
    /// Type held in the struct (inside `std::optional` for `T?`).
    storage: String,
    /// Nested refined class to generate for a `where` / `matches` on the field.
    class: Option<String>,
}

impl<'a> Emitter<'a> {
    pub fn new(ctx: &'a Ctx<'a>, src: &'a crate::diag::Source<'a>, file: &'a str) -> Self {
        Emitter { ctx, src, file, out: String::new(), indent: 0 }
    }

    pub fn type_decl(&mut self, d: &TypeDecl) -> String {
        self.out = QUIET_BEGIN.to_string();
        self.line(d.span);
        let name = mangle(&d.name.text);
        self.refined_class(&name, &d.name.text, &d.base, d.cond.as_ref(), d.matches.as_ref());
        self.out += QUIET_END;
        std::mem::take(&mut self.out)
    }

    pub fn schema_decl(&mut self, d: &SchemaDecl, inline_enums: &[&EnumDecl]) -> String {
        self.out = QUIET_BEGIN.to_string();
        for e in inline_enums {
            self.line(e.span);
            let name = mangle(&e.name.text);
            let variants: Vec<String> = e.variants.iter().map(|v| mangle(&v.text)).collect();
            self.w(&format!("enum class {name} {{ {} }};", variants.join(", ")));
            self.enum_names(e);
        }
        self.schema(d);
        self.out += QUIET_END;
        std::mem::take(&mut self.out)
    }

    /// What `cc::decode` and `cc::repr` need to know about a C++ enum a schema uses.
    pub fn enum_names_decl(&mut self, d: &EnumDecl) -> String {
        self.out.clear();
        self.enum_names(d);
        std::mem::take(&mut self.out)
    }

    fn enum_names(&mut self, d: &EnumDecl) {
        let names: Vec<String> = d.variants.iter().map(|v| cpp_str(&v.text)).collect();
        self.w(&format!(
            "inline std::vector<std::string_view> cc_enum_names({}) {{ return {{{}}}; }}",
            mangle(&d.name.text),
            names.join(", ")
        ));
    }

    fn line(&mut self, span: Span) {
        let _ = writeln!(self.out, "#line {} {}", self.src.line(span.lo), cpp_str(self.file));
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

    // ------------------------------------------------------- refined types

    fn refined_class(&mut self, cls: &str, display: &str, base: &Type, cond: Option<&Expr>, matches: Option<&Name>) {
        let b = cpp_type(base);
        let prim = match base.kind {
            TypeKind::Prim(p) => Some(p),
            _ => None,
        };
        let integer = prim.is_some_and(|p| p.is_integral() && p != Prim::Bool);
        let scalar = prim.is_some_and(|p| p != Prim::String);
        let cx = if scalar && matches.is_none() { "constexpr " } else { "" };
        let mut checks = Vec::new();
        let mut srcs = Vec::new();
        if let Some(c) = cond {
            checks.push(self.expr_prec(c, PREC_CMP));
            srcs.push(self.text(c.span).to_string());
        }
        if let Some(m) = matches {
            checks.push(format!("{}(self)", fn_name(&m.text)));
            srcs.push(format!("matches {}", m.text));
        }
        let check = match (cond, checks.len()) {
            (Some(c), 1) => self.expr(c),
            _ => checks.join(" && "),
        };
        let src = srcs.join(" && ");

        self.w(&format!("class CC_GENERATED {cls} {{"));
        self.w(" public:");
        self.indent += 1;
        self.w(&format!("using underlying_type = {b};"));
        self.w(&format!("CC_TRUSTED(\"\") static {cx}bool check(const {b}& self) {{ return {check}; }}"));
        self.w(&format!("CC_TRUSTED(\"\") static {cx}std::expected<{cls}, ::cc::RefineError> try_from({b} v) {{"));
        self.w(&format!(
            "  if (!check(v)) return std::unexpected(::cc::RefineError{{{}, {}}});",
            cpp_str(display),
            cpp_str(&src)
        ));
        self.w(&format!("  return {cls}(::cc::Unchecked{{}}, std::move(v));"));
        self.w("}");
        if integer {
            // Any integer, range-checked first: a parameter of type `b` would wrap it before `check`.
            self.w("template <::cc::integer_like A>");
            self.w(&format!("  requires(!std::same_as<A, {b}>)"));
            self.w(&format!(
                "CC_TRUSTED(\"\") static {cx}std::expected<{cls}, ::cc::RefineError> try_from(const A& a) {{"
            ));
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
        if scalar && matches.is_none() {
            // `Port p = 8080;`. Being consteval, this is the only check a literal gets,
            // and a value that is not known at compile time does not compile.
            if integer {
                self.w("template <::cc::plain_integer A>");
                self.w(&format!("consteval {cls}(A cc_literal) : v_(static_cast<{b}>(cc_literal)) {{"));
                self.w(&format!(
                    "  if (!::cc::fits<{b}>(cc_literal) || !check(static_cast<{b}>(cc_literal))) ::cc::literal_rejected();"
                ));
            } else if prim == Some(Prim::F64) {
                self.w("template <std::floating_point A>");
                self.w(&format!("consteval {cls}(A cc_literal) : v_(static_cast<{b}>(cc_literal)) {{"));
                self.w(&format!("  if (!check(static_cast<{b}>(cc_literal))) ::cc::literal_rejected();"));
            } else {
                self.w(&format!("consteval {cls}(bool cc_literal) : v_(cc_literal) {{"));
                self.w("  if (!check(cc_literal)) ::cc::literal_rejected();");
            }
            self.w("}");
        }
        self.w(&format!("CC_TRUSTED(\"\") {cx}const {b}& get() const {{ return v_; }}"));
        self.w(&format!("CC_TRUSTED(\"\") {cx}operator const {b}&() const {{ return v_; }}"));
        self.w(&format!("CC_TRUSTED(\"\") friend {cx}bool operator==(const {cls}&, const {cls}&) = default;"));
        if integer {
            // Without this `p == 8080` is ambiguous, now that 8080 also converts to the class.
            self.w("template <::cc::plain_integer A>");
            self.w(&format!(
                "CC_TRUSTED(\"\") friend {cx}bool operator==(const {cls}& a, A b) {{ return std::cmp_equal(a.v_, b); }}"
            ));
        } else if scalar {
            self.w(&format!(
                "CC_TRUSTED(\"\") friend {cx}bool operator==(const {cls}& a, const {b}& b) {{ return a.v_ == b; }}"
            ));
        } else if prim == Some(Prim::String) {
            // std::string's operator== is a template, so the conversion above does not reach it.
            self.w(&format!(
                "CC_TRUSTED(\"\") friend bool operator==(const {cls}& a, std::string_view b) {{ return a.v_ == b; }}"
            ));
        }
        self.w(&format!(
            "friend std::ostream& operator<<(std::ostream& o, const {cls}& a) {{ return o << ::cc::display(a.v_); }}"
        ));
        if matches!(base.kind, TypeKind::Vec(_)) {
            self.w("CC_TRUSTED(\"\") auto begin() const { return v_.begin(); }");
            self.w("CC_TRUSTED(\"\") auto end() const { return v_.end(); }");
            self.w("CC_TRUSTED(\"\") auto size() const { return v_.size(); }");
            self.w("CC_TRUSTED(\"\") decltype(auto) operator[](std::size_t i) const { return v_[i]; }");
        }
        self.indent -= 1;
        self.w(" private:");
        self.indent += 1;
        self.w(&format!("{cx}{cls}(::cc::Unchecked, {b} v) : v_(std::move(v)) {{}}"));
        self.w(&format!("{b} v_;"));
        self.indent -= 1;
        self.w("};");
    }

    // -------------------------------------------------------------- schemas

    fn field_plan(&self, f: &Field) -> FieldPlan {
        let inner = inner_type(&f.ty);
        if f.cond.is_some() || f.matches.is_some() {
            let cls = format!("{}_t", f.name.text);
            return FieldPlan { storage: cls.clone(), class: Some(cls) };
        }
        FieldPlan { storage: cpp_type(inner), class: None }
    }

    fn schema(&mut self, d: &SchemaDecl) {
        let name = mangle(&d.name.text);
        // A schema with an invariant must not be reachable except through parse and make,
        // and its fields must not be assignable one at a time: that would break the invariant
        // with no check. Assigning a whole, already valid value stays possible.
        let sealed = !d.invariants.is_empty();
        self.line(d.span);
        self.w(&format!("struct CC_GENERATED {name} {{"));
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
        let field_type = |f: &Field, p: &FieldPlan| {
            if is_optional(&f.ty) {
                format!("std::optional<{}>", p.storage)
            } else {
                p.storage.clone()
            }
        };
        let qualifier = if sealed { "const " } else { "" };
        for (f, p) in d.fields.iter().zip(&plans) {
            self.w(&format!("{qualifier}{} {};", field_type(f, p), mangle(&f.name.text)));
        }
        if sealed {
            self.out += "\n";
            self.w(&format!("{name}(const {name}&) = default;"));
            self.w(&format!("{name}({name}&&) = default;"));
            self.w(&format!(
                "{name}& operator=(const {name}& o) {{ if (this != &o) {{ std::destroy_at(this); std::construct_at(this, o); }} return *this; }}"
            ));
            self.w(&format!(
                "{name}& operator=({name}&& o) {{ if (this != &o) {{ std::destroy_at(this); std::construct_at(this, std::move(o)); }} return *this; }}"
            ));
        }
        for (i, inv) in d.invariants.iter().enumerate() {
            let cond = self.expr(&inv.cond);
            self.line(inv.span);
            self.w(&format!("bool cc_invariant_{i}() const {{ return {cond}; }}"));
        }

        // What `make` takes and `parse` decodes: plain values, before any checking.
        self.out += "\n";
        self.w("struct Init {");
        for (f, p) in d.fields.iter().zip(&plans) {
            self.w(&format!("  std::optional<::cc::decoded_t<{}>> {};", p.storage, mangle(&f.name.text)));
        }
        self.w("};");

        self.build(d, &plans, sealed);

        self.w(&format!(
            "static std::optional<{name}> parse_at(const ::cc::Json& j, const std::string& path, std::vector<::cc::FieldError>& errs) {{"
        ));
        self.indent += 1;
        self.w("if (!::cc::expect_object(j, path, errs)) return std::nullopt;");
        self.w("const auto cc_n = errs.size();");
        self.w("Init init;");
        self.w("std::vector<std::string_view> cc_undecodable;");
        for (f, p) in d.fields.iter().zip(&plans) {
            let key = cpp_str(&f.name.text);
            let member = mangle(&f.name.text);
            self.w(&format!("if (const auto* v = ::cc::field(j, {key})) {{"));
            self.w(&format!(
                "  init.{member} = ::cc::decode<::cc::decoded_t<{}>>(*v, ::cc::join(path, {key}), errs);",
                p.storage
            ));
            self.w(&format!("  if (!init.{member}) cc_undecodable.push_back({key});"));
            self.w("}");
        }
        if !d.open {
            let known: Vec<String> = d.fields.iter().map(|f| cpp_str(&f.name.text)).collect();
            self.w(&format!("::cc::reject_unknown(j, path, {{{}}}, errs);", known.join(", ")));
        }
        self.w("auto out = cc_build(std::move(init), path, errs, cc_undecodable);");
        let fields: Vec<String> = d.fields.iter().map(|f| cpp_str(&f.name.text)).collect();
        self.w(&format!("::cc::order_errors(errs, cc_n, path, {{{}}});", fields.join(", ")));
        self.w("if (errs.size() != cc_n) return std::nullopt;");
        self.w("return out;");
        self.indent -= 1;
        self.w("}");

        let label = cpp_str(&d.name.text);
        self.w(&format!(
            "CC_TRUSTED(\"alloc\") static std::expected<{name}, ::cc::ParseError> parse(const ::cc::Json& j) {{"
        ));
        self.w("  std::vector<::cc::FieldError> errs;");
        self.w("  auto v = parse_at(j, \"\", errs);");
        self.w(&format!("  if (!v) return std::unexpected(::cc::ParseError{{{label}, std::move(errs)}});"));
        self.w("  return std::move(*v);");
        self.w("}");
        self.w(&format!(
            "CC_TRUSTED(\"alloc\") static std::expected<{name}, ::cc::ParseError> parse(const ::cc::Raw<::cc::Json>& raw) {{ return parse(raw.unchecked()); }}"
        ));
        // The same checks as parse, for values the program itself has.
        self.w(&format!("CC_TRUSTED(\"alloc\") static std::expected<{name}, ::cc::ParseError> make(Init init) {{"));
        self.w("  std::vector<::cc::FieldError> errs;");
        self.w("  auto v = cc_build(std::move(init), \"\", errs, {});");
        self.w(&format!("  if (!v) return std::unexpected(::cc::ParseError{{{label}, std::move(errs)}});"));
        self.w("  return std::move(*v);");
        self.w("}");

        self.w("CC_TRUSTED(\"alloc\") ::cc::Json to_json() const {");
        self.w("  ::cc::Json j = ::cc::Json::object();");
        for f in &d.fields {
            self.w(&format!("  j[{}] = ::cc::encode({});", cpp_str(&f.name.text), mangle(&f.name.text)));
        }
        self.w("  return j;");
        self.w("}");
        if sealed {
            self.indent -= 1;
            self.w(" private:");
            self.indent += 1;
            let params: Vec<String> =
                d.fields.iter().zip(&plans).map(|(f, p)| format!("{} a_{}", field_type(f, p), f.name.text)).collect();
            let inits: Vec<String> =
                d.fields.iter().map(|f| format!("{}(std::move(a_{}))", mangle(&f.name.text), f.name.text)).collect();
            let colon = if inits.is_empty() { "" } else { " : " };
            self.w(&format!(
                "{name}(::cc::Unchecked{}{}){colon}{} {{}}",
                if params.is_empty() { "" } else { ", " },
                params.join(", "),
                inits.join(", ")
            ));
        }
        self.indent -= 1;
        self.w("};");
    }

    /// `cc_build`: transforms, conditions, defaults and invariants, whatever the values came from.
    fn build(&mut self, d: &SchemaDecl, plans: &[FieldPlan], sealed: bool) {
        let name = mangle(&d.name.text);
        self.w(&format!(
            "static std::optional<{name}> cc_build(Init init, const std::string& path, std::vector<::cc::FieldError>& errs, const std::vector<std::string_view>& cc_undecodable) {{"
        ));
        self.indent += 1;
        self.w("const auto cc_b = errs.size();");
        for (f, p) in d.fields.iter().zip(plans) {
            self.build_field(f, p);
        }
        self.w("if (errs.size() != cc_b) return std::nullopt;");
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
        if sealed {
            self.w(&format!(
                "{name} out{{::cc::Unchecked{{}}{}{}}};",
                if members.is_empty() { "" } else { ", " },
                members.join(", ")
            ));
        } else {
            self.w(&format!("{name} out{{{}}};", members.join(", ")));
        }
        for (i, inv) in d.invariants.iter().enumerate() {
            let src = self.text(inv.cond.span);
            let label = inv.label.as_deref().unwrap_or(src);
            self.w(&format!(
                "if (!out.cc_invariant_{i}()) errs.push_back(::cc::invariant_failure(path, {}, {}));",
                cpp_str(label),
                cpp_str(src)
            ));
        }
        if sealed {
            self.w("if (errs.size() != cc_b) return std::nullopt;");
        }
        self.w("return out;");
        self.indent -= 1;
        self.w("}");
    }

    fn build_field(&mut self, f: &Field, p: &FieldPlan) {
        let key = cpp_str(&f.name.text);
        let member = mangle(&f.name.text);
        let var = format!("f_{}", f.name.text);
        let path = format!("::cc::join(path, {key})");
        self.line(f.span);
        self.w(&format!("std::optional<{}> {var};", p.storage));
        self.w(&format!("if (init.{member}) {{"));
        if let Some(t) = &f.transform {
            self.w(&format!("  *init.{member} = {}(std::move(*init.{member}));", fn_name(&t.text)));
        }
        self.w(&format!("  {var} = ::cc::refine_into<{}>(std::move(*init.{member}), {path}, errs);", p.storage));
        match &f.default {
            Some(def) => {
                let value = match &def.kind {
                    ExprKind::List(items) => {
                        let items: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
                        format!("::cc::decoded_t<{}>{{{}}}", p.storage, items.join(", "))
                    }
                    _ => self.expr(def),
                };
                self.w(&format!("}} else if (!::cc::contains(cc_undecodable, {key})) {{"));
                self.w(&format!("  {var} = ::cc::refine_into<{}>({value}, {path}, errs);", p.storage));
                self.w("}");
            }
            None if is_optional(&f.ty) => self.w("}"),
            None => {
                self.w(&format!("}} else if (!::cc::contains(cc_undecodable, {key})) {{"));
                self.w(&format!("  ::cc::missing({path}, errs);"));
                self.w("}");
            }
        }
    }

    // ---------------------------------------------------------- expressions

    fn expr(&self, e: &Expr) -> String {
        self.expr_prec(e, 0)
    }

    fn expr_prec(&self, e: &Expr, min: u8) -> String {
        let (s, prec) = match &e.kind {
            ExprKind::Int(t) => {
                let big = t.parse::<u64>().is_ok_and(|v| v > i64::MAX as u64);
                let wide = t.parse::<u64>().is_ok_and(|v| v > i32::MAX as u64);
                let suffix = if big {
                    "ULL"
                } else if wide {
                    "LL"
                } else {
                    ""
                };
                (format!("{t}{suffix}"), PREC_ATOM)
            }
            ExprKind::Float(t) => (t.clone(), PREC_ATOM),
            ExprKind::Str { lit } => (format!("std::string({lit})"), PREC_ATOM),
            ExprKind::Bool(b) => (b.to_string(), PREC_ATOM),
            ExprKind::None => ("std::nullopt".into(), PREC_ATOM),
            ExprKind::List(items) => {
                let items: Vec<String> = items.iter().map(|i| self.expr(i)).collect();
                (format!("std::vector{{{}}}", items.join(", ")), PREC_ATOM)
            }
            ExprKind::Ident(n) => match self.ctx.variants.get(&e.span.lo) {
                Some(cpp) => (cpp.clone(), PREC_ATOM),
                None => (mangle(n), PREC_ATOM),
            },
            ExprKind::SelfValue => ("self".into(), PREC_ATOM),
            ExprKind::Path(t, m) => (format!("{}::{}", mangle(t), mangle(m)), PREC_ATOM),
            ExprKind::Field(obj, f) => (format!("{}.{}", self.expr_prec(obj, PREC_POSTFIX), mangle(f)), PREC_POSTFIX),
            ExprKind::Index(a, i) => (format!("{}[{}]", self.expr_prec(a, PREC_POSTFIX), self.expr(i)), PREC_POSTFIX),
            ExprKind::Call(callee, args) => {
                let callee_s = match &callee.kind {
                    ExprKind::Ident(n) => fn_name(n),
                    _ => self.expr_prec(callee, PREC_POSTFIX),
                };
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
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
        };
        if prec < min {
            format!("({s})")
        } else {
            s
        }
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
