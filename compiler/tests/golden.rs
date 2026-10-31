//! Tests over files in the repository:
//!
//! - compat:   examples/hand/*.cpp and tests/compat/*.cpp: `ccc` leaves them alone and they behave the same
//! - snapshot: examples/*.c.c -> tests/snapshot/<name>.cpp
//! - run:      examples/*.c.c and tests/run/*.c.c, built with g++ and run -> tests/run/expected/<name>.out
//! - errors:   tests/errors/*.c.c -> tests/errors/<name>.err
//! - tests:    examples/confcheck.c.c and tests/test/*.c.c, built with CC_TEST and run -> tests/test/expected/<name>.out
//! - effects:  `ccc check` on tests/effects, under Clang
//! - headers:  tests/headers, a .c.c file with the .c.h files it includes
//!
//! Run with UPDATE=1 to rewrite the expected files.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn files(dir: &str, ext: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root().join(dir)) else { return Vec::new() };
    let mut v: Vec<PathBuf> = rd.map(|e| e.unwrap().path()).filter(|p| p.to_string_lossy().ends_with(ext)).collect();
    v.sort();
    v
}

fn stem(p: &Path) -> String {
    let name = p.file_name().unwrap().to_string_lossy();
    name.trim_end_matches(".c.c").trim_end_matches(".cpp").to_string()
}

/// Compares `actual` with the file, or rewrites it when UPDATE is set.
fn expect(path: &Path, actual: &str) -> Result<(), String> {
    if std::env::var_os("UPDATE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, actual).unwrap();
        return Ok(());
    }
    let expected = std::fs::read_to_string(path)
        .map_err(|_| format!("{}: 期待値ファイルがありません（UPDATE=1 で作成）", path.display()))?;
    if expected == actual {
        Ok(())
    } else {
        Err(format!("{}: 出力が違います\n--- 期待\n{expected}\n--- 実際\n{actual}", path.display()))
    }
}

fn compile_file(p: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(p).unwrap();
    ccc::compile(&p.file_name().unwrap().to_string_lossy(), &text)
}

fn cxx_build(cpp: &Path, exe: &Path, extra: &[&str]) -> Result<(), String> {
    let r = root();
    let out = Command::new(std::env::var("CXX").unwrap_or("g++".into()))
        .args(["-std=c++23", "-Wall", "-Werror=return-type"])
        .args(ccc::cxx::SANITIZE_FLAGS)
        .arg(format!("-I{}", r.join("runtime").display()))
        .arg(format!("-isystem{}", r.join("third_party").display()))
        .args(extra)
        .arg(cpp)
        .arg("-o")
        .arg(exe)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| e.to_string())?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!("{}: C++ のビルドに失敗\n{stderr}", cpp.display()));
    }
    // Generated code is held to -Wall, so it never adds warnings to a user's build.
    if stderr.contains("warning:") {
        return Err(format!("{}: C++ の警告\n{stderr}", cpp.display()));
    }
    Ok(())
}

fn run_exe(exe: &Path) -> String {
    show_output(Command::new(exe).output().unwrap())
}

fn show_output(out: std::process::Output) -> String {
    let code = out.status.code().map_or("abort".to_string(), |c| c.to_string());
    format!(
        "exit: {code}\n--- stdout\n{}--- stderr\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn tmp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ccc-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Runs `f` on every input in parallel and fails with all errors together.
fn each(inputs: Vec<PathBuf>, f: impl Fn(&Path) -> Result<(), String> + Sync) {
    assert!(!inputs.is_empty(), "テスト対象のファイルがありません");
    let errors: Vec<String> = std::thread::scope(|s| {
        let handles: Vec<_> = inputs.iter().map(|p| s.spawn(|| f(p))).collect();
        handles.into_iter().filter_map(|h| h.join().unwrap().err()).collect()
    });
    assert!(errors.is_empty(), "\n{}", errors.join("\n\n"));
}

/// C++ renamed to .c.c is untouched, line for line, and does what it did.
#[test]
fn compat() {
    let dir = tmp_dir("compat");
    let inputs = [files("examples/hand", ".cpp"), files("tests/compat", ".cpp")].concat();
    each(inputs, |p| {
        let text = std::fs::read_to_string(p).unwrap();
        let name = format!("{}.c.c", stem(p));
        let cpp = ccc::compile(&name, &text)?;
        if !cpp.ends_with(&text) {
            return Err(format!("{}: C++ の部分が変わりました\n{cpp}", p.display()));
        }
        let direct = dir.join(format!("direct_{}", stem(p)));
        let through = dir.join(format!("through_{}", stem(p)));
        let through_cpp = dir.join(format!("through_{}.cpp", stem(p)));
        std::fs::write(&through_cpp, cpp).unwrap();
        cxx_build(p, &direct, &[])?;
        cxx_build(&through_cpp, &through, &[])?;
        let (a, b) = (run_exe(&direct), run_exe(&through));
        let normalize = |s: &str| s.replace(&format!("{}.cpp", stem(p)), "FILE").replace(&name, "FILE");
        if normalize(&a) != normalize(&b) {
            return Err(format!("{}: 結果が変わりました\n--- 元\n{a}\n--- ccc\n{b}", p.display()));
        }
        Ok(())
    });
}

#[test]
fn snapshot() {
    each(files("examples", ".c.c"), |p| {
        let cpp = compile_file(p)?;
        expect(&root().join("tests/snapshot").join(format!("{}.cpp", stem(p))), &cpp)
    });
}

#[test]
fn run() {
    let dir = tmp_dir("run");
    let inputs = [files("examples", ".c.c"), files("tests/run", ".c.c")].concat();
    each(inputs, |p| {
        let cpp = compile_file(p)?;
        let cpp_path = dir.join(format!("{}.cpp", stem(p)));
        let exe = dir.join(stem(p));
        std::fs::write(&cpp_path, cpp).unwrap();
        cxx_build(&cpp_path, &exe, &[])?;
        expect(&root().join("tests/run/expected").join(format!("{}.out", stem(p))), &run_exe(&exe))
    });
}

/// examples/confcheck.c.c takes a file argument, which `run` cannot pass.
#[test]
fn confcheck_cli() {
    let dir = tmp_dir("confcheck");
    let cpp_path = dir.join("confcheck.cpp");
    let exe = dir.join("confcheck");
    std::fs::write(&cpp_path, compile_file(&root().join("examples/confcheck.c.c")).unwrap()).unwrap();
    cxx_build(&cpp_path, &exe, &[]).unwrap();
    let configs = root().join("examples/confcheck");
    let mut errors = Vec::new();
    for name in ["good", "bad", "duplicate", "missing"] {
        let out = Command::new(&exe).arg(format!("{name}.json")).current_dir(&configs).output().unwrap();
        let expected = root().join("tests/run/expected").join(format!("confcheck_{name}.out"));
        if let Err(e) = expect(&expected, &show_output(out)) {
            errors.push(e);
        }
    }
    assert!(errors.is_empty(), "\n{}", errors.join("\n\n"));
}

/// `test` blocks, built the way `ccc test` builds them.
#[test]
fn test_blocks() {
    let dir = tmp_dir("tests");
    let inputs =
        [vec![root().join("examples/confcheck.c.c"), root().join("examples/effects.c.c")], files("tests/test", ".c.c")]
            .concat();
    each(inputs, |p| {
        let cpp = compile_file(p)?;
        let cpp_path = dir.join(format!("{}.cpp", stem(p)));
        let exe = dir.join(stem(p));
        std::fs::write(&cpp_path, cpp).unwrap();
        cxx_build(&cpp_path, &exe, &["-DCC_TEST"])?;
        expect(&root().join("tests/test/expected").join(format!("{}.out", stem(p))), &run_exe(&exe))
    });
}

#[test]
fn errors() {
    each(files("tests/errors", ".c.c"), |p| match compile_file(p) {
        Ok(_) => Err(format!("{}: エラーになるはずが変換に成功しました", p.display())),
        Err(msg) => expect(&p.with_file_name(format!("{}.err", stem(p))), &msg),
    });
}

/// Runs the `ccc` binary on an existing file. Returns the exit code, stdout and stderr.
fn ccc_on(cmd: &str, file: &Path, extra_env: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_ccc"));
    c.arg(cmd).arg(file);
    for (k, v) in extra_env {
        c.env(k, v);
    }
    let out = c.output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// Runs the `ccc` binary with `source` in its own directory, which is also its TMPDIR.
/// Returns the exit code and stderr, after checking that no temporary files stayed behind.
fn ccc(name: &str, cmd: &str, source: &str) -> (Option<i32>, String) {
    let dir = tmp_dir(&format!("cli-{name}"));
    let work = dir.join("tmp");
    std::fs::create_dir_all(&work).unwrap();
    let file = dir.join(format!("{name}.c.c"));
    std::fs::write(&file, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ccc")).arg(cmd).arg(&file).env("TMPDIR", &work).output().unwrap();
    let left: Vec<_> = std::fs::read_dir(&work).unwrap().collect();
    assert!(left.is_empty(), "{name}: 一時ファイルが残っています: {left:?}");
    (out.status.code(), String::from_utf8_lossy(&out.stderr).to_string())
}

/// What `ccc check` finds through the C++ compiler, and how it is shown.
#[test]
fn check_reports_cpp_errors_against_the_source() {
    let port = "#include <cc/cc.hpp>\ntype Port = u16 where 1 <= self <= 65535;\n";
    let strict = "#pragma cc strict\n#include <cstdint>\n";
    let cases = [
        ("undefined", "int main() { return nope; }\n".to_string(), "'nope'".to_string()),
        ("mismatch", "int main() { int x = \"a\"; }\n".to_string(), "invalid conversion".to_string()),
        (
            "literal",
            format!("{port}int main() {{ Port p = 0; }}\n"),
            "literal.c.c:3: エラー: リテラル 0 は Port の条件を満たしていません".to_string(),
        ),
        (
            "literal_range",
            format!("{port}int main() {{\n  Port p = 70000;\n}}\n"),
            "literal_range.c.c:4: エラー: リテラル 70000 は Port の条件を満たしていません".to_string(),
        ),
        (
            "runtime_value",
            format!("{port}int f(int n) {{ Port p = n + 1; return p; }}\nint main() {{}}\n"),
            "T::try_from".to_string(),
        ),
        (
            "try_from_float",
            format!("{port}int main() {{ double d = 8080.5; (void)Port::try_from(d); }}\n"),
            "deleted".to_string(),
        ),
        (
            "invariant_field",
            "#include <cc/cc.hpp>\nschema S {\n  a: i64;\n  invariant(a > 0);\n}\nvoid f(S& s) { s.a = 5; }\nint main() {}\n"
                .to_string(),
            "read-only".to_string(),
        ),
        (
            "raw_as_user",
            "#include <cc/cc.hpp>\nschema U { a: i64; }\nvoid g(const U&);\nvoid f(cc::Raw<cc::Json> b) { g(b); }\nint main() {}\n"
                .to_string(),
            "cc::Raw".to_string(),
        ),
        (
            "strict_narrow",
            format!("{strict}int main() {{ std::int64_t n = 73616; std::uint16_t q = n; return q; }}\n"),
            "may change value".to_string(),
        ),
        (
            "strict_sign",
            format!("{strict}int main() {{ int a = -1; unsigned b = 1; return a < b; }}\n"),
            "comparison".to_string(),
        ),
        (
            "strict_return",
            format!("{strict}// 3 行目の関数\nint f(int a) {{\n  if (a > 0) {{\n    return 1;\n  }}\n}}\nint main() {{ return f(1); }}\n"),
            "control reaches end".to_string(),
        ),
    ];
    let errors: Vec<String> = cases
        .iter()
        .filter_map(|(name, source, needle)| {
            let (code, err) = ccc(name, "check", source);
            let hidden = ["ccc-run-", "^~"].iter().any(|h| err.contains(h));
            let located = err.contains(&format!("{name}.c.c:"));
            (code != Some(1) || !err.contains(needle.as_str()) || hidden || !located)
                .then(|| format!("{name}: exit {code:?}, `{needle}` を期待\n{err}"))
        })
        .collect();
    assert!(errors.is_empty(), "\n{}", errors.join("\n"));
}

/// Without `#pragma cc strict`, C++ means what it means.
#[test]
fn check_accepts_what_cpp_accepts() {
    let cases = [
        ("valid", "#include <cstdio>\nint main() { std::puts(\"x\"); return 0; }\n"),
        (
            "narrowing",
            "#include <cstdint>\nint main() { std::int64_t n = 73616; std::uint16_t q = n; return q - q; }\n",
        ),
        ("no_return", "int f(int a) { if (a > 0) { return 1; } }\nint main() { return f(1) - 1; }\n"),
    ];
    for (name, source) in cases {
        let (code, err) = ccc(name, "check", source);
        assert_eq!(code, Some(0), "{name}: {err}");
    }
}

/// A signal other than the abort of a contract violation must not end the program silently.
#[test]
fn run_reports_death_by_signal() {
    let source = "#include <csignal>\nint main() { std::raise(SIGSEGV); return 0; }\n";
    let (code, err) = ccc("signal", "run", source);
    assert_eq!(code, Some(139), "{err}");
    assert!(err.contains("シグナル 11"), "{err}");
}

/// A literal that breaks the condition does not compile.
#[test]
fn hand_literal_is_checked_at_compile_time() {
    let dir = tmp_dir("hand-fail");
    let r = cxx_build(&root().join("examples/hand/port.cpp"), &dir.join("port"), &["-DCC_EXPECT_COMPILE_ERROR"]);
    let err = r.expect_err("Port bad = 0; がコンパイルできてしまいました");
    assert!(err.contains("literal_rejected"), "{err}");
}

/// Line numbers survive the transform, in reports and in compiler errors alike.
#[test]
fn positions_are_the_source_positions() {
    let source = "#include <cc/cc.hpp>\ntype Port = u16 where self > 0; int after_on_the_same_line = nope;\n";
    let (code, err) = ccc("same_line", "check", source);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("same_line.c.c:2: エラー:"), "{err}");

    let source =
        "#include <cc/cc.hpp>\nint f(int x)\n  pre(x > 0,\n      \"正\")\n{\n  return\n    nope;\n}\nint main() {}\n";
    let (code, err) = ccc("in_body", "check", source);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("in_body.c.c:7: エラー:"), "{err}");
}

/// .c.h files, generated afresh next to the C++ on every run.
#[test]
fn extended_headers() {
    let main = root().join("tests/headers/main.c.c");
    let (code, out, err) = ccc_on("run", &main, &[]);
    expect(&root().join("tests/headers/main.out"), &format!("exit: {code:?}\n--- stdout\n{out}--- stderr\n{err}"))
        .unwrap();

    let dir = tmp_dir("headers-build");
    let out_cpp = dir.join("main.cpp");
    let status = Command::new(env!("CARGO_BIN_EXE_ccc"))
        .args(["build", main.to_str().unwrap(), "-o", out_cpp.to_str().unwrap()])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(dir.join("shared.h").exists() && dir.join("nested.h").exists());
    let exe = dir.join("main");
    cxx_build(&out_cpp, &exe, &[&format!("-iquote{}", root().join("tests/headers").display())]).unwrap();

    // Refuses to overwrite a file it did not write.
    std::fs::write(dir.join("shared.h"), "// mine\n").unwrap();
    let again = Command::new(env!("CARGO_BIN_EXE_ccc"))
        .args(["build", main.to_str().unwrap(), "-o", out_cpp.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert_eq!(std::fs::read_to_string(dir.join("shared.h")).unwrap(), "// mine\n");
}

fn clang_available() -> bool {
    let candidates = ["clang++-22", "clang++-21", "clang++-20", "clang++-19", "clang++"];
    let found = candidates.iter().any(|c| Command::new(c).arg("--version").output().is_ok_and(|o| o.status.success()));
    assert!(found || std::env::var_os("CI").is_none(), "CI には Clang が必要です");
    found
}

#[test]
fn effects() {
    if !clang_available() {
        eprintln!("Clang がないので効果の検査のテストを飛ばします");
        return;
    }
    let (code, _, err) = ccc_on("check", &root().join("tests/effects/clean.c.c"), &[]);
    assert_eq!(code, Some(0), "{err}");
    for example in ["examples/confcheck.c.c", "examples/effects.c.c"] {
        let (code, _, err) = ccc_on("check", &root().join(example), &[]);
        assert_eq!(code, Some(0), "{example}: {err}");
    }

    let (code, _, err) = ccc_on("check", &root().join("tests/effects/violations.c.c"), &[]);
    assert_eq!(code, Some(1), "{err}");
    expect(&root().join("tests/effects/violations.err"), &err).unwrap();

    let (code, _, err) = ccc_on("check", &root().join("tests/headers/bad_effect.c.c"), &[]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("bad_effect.c.c:3:") && err.contains("shout"), "{err}");
}

/// Adding `cc::print` to a function declared pure must be caught: PLAN.md step 7.
#[test]
fn a_print_added_to_confcheck_is_reported() {
    if !clang_available() {
        return;
    }
    let text = std::fs::read_to_string(root().join("examples/confcheck.c.c")).unwrap();
    let changed = text.replace("std::int64_t sum = 0;", "std::int64_t sum = 0; cc::print(\"debug\");");
    assert_ne!(text, changed);
    let (code, err) = ccc("confcheck_debug", "check", &changed);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("cc::print") && err.contains("total_weight"), "{err}");
}

/// Without Clang the check is skipped with a warning rather than passing in silence.
#[test]
fn effects_without_clang_warn() {
    let dir = tmp_dir("no-clang");
    let file = dir.join("f.c.c");
    std::fs::write(&file, "[[cc::effects()]] int f() { return 0; }\nint main() { return f(); }\n").unwrap();
    // A PATH holding only what g++ needs.
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let which = |name: &str| {
        let out = Command::new("which").arg(name).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    for tool in ["as", "ld"] {
        let _ = std::os::unix::fs::symlink(which(tool), bin.join(tool));
    }
    let out = Command::new(env!("CARGO_BIN_EXE_ccc"))
        .arg("check")
        .arg(&file)
        .env("PATH", &bin)
        .env("CXX", which("g++"))
        .env_remove("CC_CLANG")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("Clang が見つからない"), "{err}");
}
