//! Driving the C++ compiler: its flags, and turning its output into something
//! that talks about the .c.c file.

/// Added to `ccc run` and `ccc test`. Signed overflow, division by zero and the
/// like stop the program with a message instead of being undefined behaviour.
pub const SANITIZE_FLAGS: &[&str] = &["-fsanitize=undefined", "-fno-sanitize-recover=undefined"];

const MAX_ERRORS: usize = 10;

/// The diagnostics of a failed C++ build, rendered against the .c.c source.
///
/// Only `error:` lines are kept. Their columns are dropped because `#line` maps
/// lines only: a column is a position in the generated C++, not in the source.
/// Returns the output unchanged when no such line exists (a linker error, say).
pub fn summarize(stderr: &str, file: &str, text: &str) -> String {
    let mut seen: Vec<(Option<usize>, String)> = Vec::new();
    // An error inside a header is reported against the last place in the .c.c file that led there.
    let mut last_line = None;
    for line in stderr.lines() {
        let located = source_line(line, file);
        if located.is_some() {
            last_line = located;
        }
        let Some((_, message)) = line.split_once(": error: ").or_else(|| line.split_once(": fatal error: ")) else {
            continue;
        };
        if line.starts_with("collect2:") {
            continue;
        }
        let entry = (located.or(last_line), clean(message));
        if !seen.contains(&entry) {
            seen.push(entry);
        }
    }
    if seen.is_empty() {
        return stderr.to_string();
    }
    // A rejected literal also reports the narrowing behind it, which says nothing new.
    let literal_lines: Vec<Option<usize>> =
        seen.iter().filter(|(_, m)| m.starts_with("リテラル")).map(|(l, _)| *l).collect();
    seen.retain(|(l, m)| !(m.starts_with("narrowing conversion") && literal_lines.contains(l)));

    let sources: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    for (line, message) in seen.iter().take(MAX_ERRORS) {
        match line {
            Some(n) => {
                out += &format!("{file}:{n}: エラー: {message}\n");
                if let Some(s) = n.checked_sub(1).and_then(|i| sources.get(i)) {
                    out += &format!("{:w$} |\n{n} | {s}\n", "", w = n.to_string().len());
                }
            }
            None => out += &format!("{file}: エラー: {message}\n"),
        }
    }
    if seen.len() > MAX_ERRORS {
        out += &format!("ほか {} 件\n", seen.len() - MAX_ERRORS);
    }
    out
}

/// The line number when `line` starts with `file:LINE:`, whatever directory `file` is in.
fn source_line(line: &str, file: &str) -> Option<usize> {
    let mut parts = line.splitn(3, ':');
    let path = parts.next()?;
    let number = parts.next()?;
    (path.rsplit('/').next() == Some(file)).then(|| number.parse().ok()).flatten()
}

fn clean(message: &str) -> String {
    let message = match message.rfind(" [-W") {
        Some(i) if message.ends_with(']') => &message[..i],
        _ => message,
    };
    literal_rejected(message).unwrap_or_else(|| message.to_string())
}

/// `no matching function for call to 'Port::of<0>()'` is how C++ words a rejected literal.
fn literal_rejected(message: &str) -> Option<String> {
    let call = message.strip_prefix("no matching function for call to '")?.strip_suffix("()'")?;
    let (ty, value) = call.split_once("::of<")?;
    let value = value.strip_suffix('>')?;
    Some(format!("リテラル {value} は {ty} の条件を満たしていません"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "type Port = u16 where self > 0;\nlet x: i64 = \"a\";\n";

    #[test]
    fn keeps_errors_and_drops_notes_columns_and_duplicates() {
        let stderr = "a.c.c: In function 'int cc_main()':\n\
                      a.c.c:2:25: error: cannot convert 'std::string' to 'int64_t' in initialization\n    \
                      2 | let x\n      |  ^~~\n\
                      a.c.c:2:9: error: cannot convert 'std::string' to 'int64_t' in initialization\n\
                      a.c.c:1:3: note: candidate\n\
                      a.c.c:1:3: warning: something\n";
        assert_eq!(
            summarize(stderr, "a.c.c", SOURCE),
            "a.c.c:2: エラー: cannot convert 'std::string' to 'int64_t' in initialization\n  |\n2 | let x: i64 = \"a\";\n"
        );
    }

    #[test]
    fn matches_the_file_in_any_directory_and_hides_the_flag() {
        let stderr = "/tmp/ccc-run-1/../a.c.c:1:1: error: conversion may change value [-Werror=conversion]\n";
        assert!(summarize(stderr, "a.c.c", SOURCE).starts_with("a.c.c:1: エラー: conversion may change value\n"));
    }

    #[test]
    fn error_in_a_header_points_at_the_source_line_that_led_there() {
        let stderr = "a.c.c:2:7:   required from here\n/usr/include/x.h:9:1: error: static assertion failed\n";
        assert!(summarize(stderr, "a.c.c", SOURCE).starts_with("a.c.c:2: エラー: static assertion failed\n"));
    }

    #[test]
    fn explains_a_rejected_literal() {
        let stderr = "a.c.c:2:1: error: no matching function for call to 'Port::of<70000>()'\n\
                      a.c.c:2:1: error: narrowing conversion of '70000' from 'int' to 'short unsigned int' [-Wnarrowing]\n";
        let out = summarize(stderr, "a.c.c", SOURCE);
        assert!(out.starts_with("a.c.c:2: エラー: リテラル 70000 は Port の条件を満たしていません\n"), "{out}");
        assert!(!out.contains("narrowing"), "{out}");
    }

    #[test]
    fn leaves_a_linker_error_alone() {
        let stderr = "/usr/bin/ld: x.o: undefined reference to `f()'\ncollect2: error: ld returned 1 exit status\n";
        assert_eq!(summarize(stderr, "a.c.c", SOURCE), stderr);
    }

    #[test]
    fn caps_the_number_of_errors() {
        let stderr: String = (0..12).map(|i| format!("a.c.c:1:1: error: e{i}\n")).collect();
        assert!(summarize(&stderr, "a.c.c", SOURCE).ends_with("ほか 2 件\n"));
    }
}
