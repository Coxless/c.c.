//! ccc: the C.C. transpiler. A .c.c file is C++ with a few extensions; `ccc`
//! replaces those and leaves every other character as it was.

pub mod ast;
pub mod check;
pub mod cxx;
pub mod diag;
pub mod effect_table;
pub mod effects;
pub mod emit;
pub mod lexer;
pub mod parser;
pub mod rewrite;
pub mod scan;

/// Transpiles one .c.c file. On failure returns the rendered diagnostics.
pub fn compile(name: &str, text: &str) -> Result<String, String> {
    rewrite::transform(name, text, rewrite::Mode::Build, false).map(|o| o.cpp)
}
