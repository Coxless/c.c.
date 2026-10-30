//! Diagnostics with file:line:col and a caret under the offending source.

use crate::ast::Span;

#[derive(Clone, Debug)]
pub struct Diag {
    pub span: Span,
    pub msg: String,
    pub note: Option<String>,
}

impl Diag {
    pub fn new(span: Span, msg: impl Into<String>) -> Diag {
        Diag { span, msg: msg.into(), note: None }
    }

    pub fn note(mut self, note: impl Into<String>) -> Diag {
        self.note = Some(note.into());
        self
    }
}

pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> Source<'a> {
    pub fn new(name: &'a str, text: &'a str) -> Source<'a> {
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        Source { name, text, line_starts }
    }

    /// 1-based line number.
    pub fn line(&self, pos: usize) -> usize {
        self.line_starts.partition_point(|&s| s <= pos)
    }

    /// 1-based line and column (in characters).
    pub fn line_col(&self, pos: usize) -> (usize, usize) {
        let line = self.line(pos);
        let start = self.line_starts[line - 1];
        (line, self.text[start..pos].chars().count() + 1)
    }

    pub fn slice(&self, span: Span) -> &'a str {
        &self.text[span.lo..span.hi]
    }

    pub fn render(&self, d: &Diag) -> String {
        let (line, col) = self.line_col(d.span.lo);
        let start = self.line_starts[line - 1];
        let end = self.text[start..].find('\n').map_or(self.text.len(), |i| start + i);
        let src_line = &self.text[start..end];
        let hi = d.span.hi.clamp(d.span.lo, end);
        let width = self.text[d.span.lo..hi].chars().count().max(1);
        let gutter = line.to_string().len();
        let mut out = format!("{}:{}:{}: エラー: {}\n", self.name, line, col, d.msg);
        out += &format!("{:gutter$} |\n{} | {}\n", "", line, src_line);
        out += &format!("{:gutter$} | {}{}\n", "", " ".repeat(col - 1), "^".repeat(width));
        if let Some(n) = &d.note {
            out += &format!("{:gutter$} = 補足: {}\n", "", n);
        }
        out
    }
}
