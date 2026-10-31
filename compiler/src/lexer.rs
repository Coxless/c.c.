//! Tokenizer for the inside of `type` and `schema` declarations. The scanner in
//! `scan.rs` finds where those declarations are; this only ever sees their text.

use crate::ast::Span;
use crate::diag::Diag;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(String),
    Float(String),
    /// `lit` is the C++ spelling, `value` the decoded text.
    Str {
        lit: String,
        value: String,
    },
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

const PUNCTS: &[&str] = &[
    "::", "->", "..", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "(", ")", "{", "}", "[", "]",
    "<", ">", ",", ";", ":", ".", "?", "=", "+", "-", "*", "/", "%", "!", "&",
];

/// Tokens of `src[lo..hi]`. Their spans are positions in the whole of `src`.
pub fn lex(src: &str, lo: usize, hi: usize) -> Result<Vec<Token>, Diag> {
    let src = &src[..hi];
    Lexer { src, b: src.as_bytes(), pos: lo, out: Vec::new() }.run()
}

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    out: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn peek(&self, off: usize) -> u8 {
        self.b.get(self.pos + off).copied().unwrap_or(0)
    }

    fn push(&mut self, tok: Tok, lo: usize) {
        self.out.push(Token { tok, span: Span { lo, hi: self.pos } });
    }

    fn run(mut self) -> Result<Vec<Token>, Diag> {
        loop {
            self.skip_trivia()?;
            let lo = self.pos;
            let c = self.peek(0);
            if c == 0 {
                self.push(Tok::Eof, lo);
                return Ok(self.out);
            }
            if c == b'R' && self.peek(1) == b'"' {
                self.pos += 1;
                let (lit, value) = self.raw_string(lo)?;
                self.push(Tok::Str { lit: format!("R{lit}"), value }, lo);
            } else if c.is_ascii_alphabetic() || c == b'_' {
                while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                    self.pos += 1;
                }
                let word = &self.src[lo..self.pos];
                self.push(Tok::Ident(word.to_string()), lo);
            } else if c.is_ascii_digit() {
                self.number(lo);
            } else if c == b'"' {
                let (lit, value) = self.string(lo)?;
                self.push(Tok::Str { lit, value }, lo);
            } else if let Some(p) = PUNCTS.iter().find(|p| self.src[lo..].starts_with(**p)) {
                self.pos += p.len();
                self.push(Tok::Punct(p), lo);
            } else {
                let ch = self.src[lo..].chars().next().unwrap();
                return Err(Diag::new(Span { lo, hi: lo + ch.len_utf8() }, format!("使えない文字 `{ch}`")));
            }
        }
    }

    fn skip_trivia(&mut self) -> Result<(), Diag> {
        loop {
            match (self.peek(0), self.peek(1)) {
                (c, _) if c.is_ascii_whitespace() => self.pos += 1,
                (b'/', b'/') => {
                    while !matches!(self.peek(0), b'\n' | 0) {
                        self.pos += 1;
                    }
                }
                (b'/', b'*') => {
                    let lo = self.pos;
                    match self.src[self.pos + 2..].find("*/") {
                        Some(i) => self.pos += i + 4,
                        None => return Err(Diag::new(Span { lo, hi: lo + 2 }, "コメント /* が閉じていません")),
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn number(&mut self, lo: usize) {
        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X' | b'b' | b'B') {
            self.pos += 2;
            while self.peek(0).is_ascii_hexdigit() || self.peek(0) == b'\'' {
                self.pos += 1;
            }
            let text = self.src[lo..self.pos].to_string();
            return self.push(Tok::Int(text), lo);
        }
        while self.peek(0).is_ascii_digit() || self.peek(0) == b'\'' {
            self.pos += 1;
        }
        let mut float = false;
        if self.peek(0) == b'.' && self.peek(1).is_ascii_digit() {
            float = true;
            self.pos += 1;
            while self.peek(0).is_ascii_digit() || self.peek(0) == b'\'' {
                self.pos += 1;
            }
        }
        if matches!(self.peek(0), b'e' | b'E')
            && (self.peek(1).is_ascii_digit() || (matches!(self.peek(1), b'+' | b'-') && self.peek(2).is_ascii_digit()))
        {
            float = true;
            self.pos += 2;
            while self.peek(0).is_ascii_digit() {
                self.pos += 1;
            }
        }
        let text = self.src[lo..self.pos].to_string();
        self.push(if float { Tok::Float(text) } else { Tok::Int(text) }, lo);
    }

    fn string(&mut self, lo: usize) -> Result<(String, String), Diag> {
        self.pos += 1;
        let mut value = String::new();
        loop {
            let rest = &self.src[self.pos..];
            let Some(ch) = rest.chars().next() else {
                return Err(Diag::new(Span { lo, hi: lo + 1 }, "文字列が閉じていません"));
            };
            match ch {
                '"' => {
                    self.pos += 1;
                    return Ok((self.src[lo..self.pos].to_string(), value));
                }
                '\n' => return Err(Diag::new(Span { lo, hi: self.pos }, "文字列が閉じていません")),
                '\\' => {
                    let esc = rest[1..].chars().next().unwrap_or(' ');
                    value.push(match esc {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        '\\' | '"' | '\'' => esc,
                        _ => {
                            let s = Span { lo: self.pos, hi: self.pos + 1 + esc.len_utf8() };
                            return Err(Diag::new(s, format!("未対応のエスケープ `\\{esc}`")));
                        }
                    });
                    self.pos += 1 + esc.len_utf8();
                }
                _ => {
                    value.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
    }

    /// `"delim( ... )delim"`, after the leading `R`.
    fn raw_string(&mut self, lo: usize) -> Result<(String, String), Diag> {
        let start = self.pos;
        let Some(open) = self.src[start..].find('(') else {
            return Err(Diag::new(Span { lo, hi: start + 1 }, "生文字列 R\"(...)\" の書き方が不正です"));
        };
        let delim = &self.src[start + 1..start + open];
        let close = format!("){delim}\"");
        let body_lo = start + open + 1;
        let Some(end) = self.src[body_lo..].find(&close) else {
            return Err(Diag::new(Span { lo, hi: body_lo }, "生文字列が閉じていません"));
        };
        let value = self.src[body_lo..body_lo + end].to_string();
        self.pos = body_lo + end + close.len();
        Ok((self.src[start..self.pos].to_string(), value))
    }
}
