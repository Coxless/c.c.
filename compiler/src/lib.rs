//! ccc: the C.C. to C++ transpiler.

pub mod ast;
pub mod check;
pub mod cxx;
pub mod diag;
pub mod emit;
pub mod lexer;
pub mod parser;

use diag::Source;

/// Transpiles one .c.c file. On failure returns the rendered diagnostics.
pub fn compile(name: &str, text: &str) -> Result<String, String> {
    let src = Source::new(name, text);
    let program = lexer::lex(text).and_then(parser::parse).map_err(|d| src.render(&d))?;
    let ctx =
        check::check(&program, &src).map_err(|ds| ds.iter().map(|d| src.render(d)).collect::<Vec<_>>().join("\n"))?;
    Ok(emit::emit(&program, &ctx, &src))
}
