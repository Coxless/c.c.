//! A scanner for C++ text. It does not understand C++: it only splits a file into
//! tokens well enough that comments, strings and preprocessor lines can never be
//! mistaken for an extension, and that brackets can be matched.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    Ident,
    Num,
    Str,
    Char,
    Punct,
    /// A whole preprocessor line, with its continuations.
    Directive,
}

#[derive(Clone, Copy, Debug)]
pub struct T {
    pub k: K,
    pub lo: usize,
    pub hi: usize,
}

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c >= 0x80
}

fn ident_char(c: u8) -> bool {
    ident_start(c) || c.is_ascii_digit()
}

pub fn scan(src: &str) -> Vec<T> {
    let b = src.as_bytes();
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut i = 0;
    // Only whitespace and comments so far on this line, so a `#` starts a directive.
    let mut line_start = true;
    while i < b.len() {
        let c = b[i];
        match c {
            b'\n' => {
                line_start = true;
                i += 1;
                continue;
            }
            _ if c.is_ascii_whitespace() => {
                i += 1;
                continue;
            }
            b'/' if at(i + 1) == b'/' => {
                i = line_comment_end(b, i);
                continue;
            }
            b'/' if at(i + 1) == b'*' => {
                i = block_comment_end(src, i);
                continue;
            }
            b'#' if line_start => {
                let hi = directive_end(b, i);
                out.push(T { k: K::Directive, lo: i, hi });
                i = hi;
                continue;
            }
            _ => {}
        }
        line_start = false;
        let lo = i;
        if ident_start(c) {
            while ident_char(at(i)) {
                i += 1;
            }
            let word = &src[lo..i];
            if at(i) == b'"' && matches!(word, "R" | "u8R" | "uR" | "UR" | "LR") {
                i = raw_string_end(src, i);
                out.push(T { k: K::Str, lo, hi: i });
            } else if at(i) == b'"' && matches!(word, "u8" | "u" | "U" | "L") {
                i = quoted_end(b, i);
                out.push(T { k: K::Str, lo, hi: i });
            } else if at(i) == b'\'' && matches!(word, "u8" | "u" | "U" | "L") {
                i = quoted_end(b, i);
                out.push(T { k: K::Char, lo, hi: i });
            } else {
                out.push(T { k: K::Ident, lo, hi: i });
            }
        } else if c.is_ascii_digit() || (c == b'.' && at(i + 1).is_ascii_digit()) {
            i = number_end(b, i);
            out.push(T { k: K::Num, lo, hi: i });
        } else if c == b'"' {
            i = quoted_end(b, i);
            out.push(T { k: K::Str, lo, hi: i });
        } else if c == b'\'' {
            i = quoted_end(b, i);
            out.push(T { k: K::Char, lo, hi: i });
        } else {
            i += match (c, at(i + 1), at(i + 2)) {
                (b':', b':', _) | (b'-', b'>', _) => 2,
                (b'.', b'.', b'.') => 3,
                _ => 1,
            };
            out.push(T { k: K::Punct, lo, hi: i });
        }
    }
    out
}

fn line_comment_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i] != b'\n' {
        // A backslash at the end of the line continues the comment.
        if b[i] == b'\\' && b.get(i + 1) == Some(&b'\n') {
            i += 1;
        }
        i += 1;
    }
    i
}

fn block_comment_end(src: &str, i: usize) -> usize {
    src[i + 2..].find("*/").map_or(src.len(), |n| i + 2 + n + 2)
}

/// The end of the line, following `\` continuations and comments that span lines.
fn directive_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        match b[i] {
            b'\n' => return i,
            b'\\' if b.get(i + 1) == Some(&b'\n') => i += 2,
            b'\\' if b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n') => i += 3,
            b'/' if b.get(i + 1) == Some(&b'/') => return line_comment_end(b, i),
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut j = i + 2;
                while j + 1 < b.len() && !(b[j] == b'*' && b[j + 1] == b'/') {
                    j += 1;
                }
                i = (j + 2).min(b.len());
            }
            // An unterminated `"` ends with the line, like everything here.
            b'"' => {
                i += 1;
                while i < b.len() && b[i] != b'"' && b[i] != b'\n' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                if b.get(i) == Some(&b'"') {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    b.len()
}

/// `i` is at the opening quote. An unterminated literal ends with its line: the
/// C++ compiler reports it better than this could.
fn quoted_end(b: &[u8], mut i: usize) -> usize {
    let q = b[i];
    i += 1;
    while i < b.len() {
        match b[i] {
            c if c == q => return i + 1,
            b'\n' => return i,
            b'\\' => i += 2,
            _ => i += 1,
        }
    }
    b.len()
}

/// `i` is at the `"` of `R"delim( ... )delim"`.
fn raw_string_end(src: &str, i: usize) -> usize {
    let Some(open) = src[i..].find('(') else { return src.len() };
    let delim = &src[i + 1..i + open];
    let close = format!("){delim}\"");
    src[i + open..].find(&close).map_or(src.len(), |n| i + open + n + close.len())
}

/// A pp-number, which is what makes `1'000'000` and `0x1p-3` one token.
fn number_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        match b[i] {
            b'e' | b'E' | b'p' | b'P' if matches!(b.get(i + 1), Some(b'+' | b'-')) => i += 2,
            b'\'' if b.get(i + 1).is_some_and(|c| ident_char(*c)) => i += 2,
            c if ident_char(c) || c == b'.' => i += 1,
            _ => break,
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(src: &str) -> Vec<&str> {
        scan(src).iter().map(|t| &src[t.lo..t.hi]).collect()
    }

    #[test]
    fn digit_separators_are_not_character_literals() {
        assert_eq!(words("int x = 1'000'000; { }"), ["int", "x", "=", "1'000'000", ";", "{", "}"]);
    }

    #[test]
    fn strings_and_comments_hide_their_contents() {
        assert_eq!(words(r#"a /* { */ "}" // {"#), ["a", "\"}\""]);
        assert_eq!(words("R\"x(}\")\")x\" b"), ["R\"x(}\")\")x\"", "b"]);
        assert_eq!(words(r#"'{' u8"}" '\''"#), ["'{'", "u8\"}\"", r"'\''"]);
    }

    #[test]
    fn directives_are_single_tokens() {
        let src = "#define X { \\\n  }\n  # include <a>\nb #c\n";
        let toks = scan(src);
        assert_eq!(toks[0].k, K::Directive);
        assert_eq!(&src[toks[0].lo..toks[0].hi], "#define X { \\\n  }");
        assert_eq!(&src[toks[1].lo..toks[1].hi], "# include <a>");
        assert_eq!(toks[3].k, K::Punct, "`#` in the middle of a line is not a directive");
    }

    #[test]
    fn a_comment_before_a_directive_keeps_it_a_directive() {
        let src = "/* c */ #if 1\n";
        assert_eq!(scan(src)[0].k, K::Directive);
    }
}
