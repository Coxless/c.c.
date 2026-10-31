//! Golden tests over files in the repository:
//!
//! - snapshot: examples/*.c.c and tests/snapshot/*.c.c -> tests/snapshot/<name>.cpp
//! - run:      examples/*.c.c and tests/run/*.c.c, built with g++ and run -> tests/run/expected/<name>.out
//! - errors:   tests/errors/*.c.c -> tests/errors/<name>.err
//! - hand:     examples/hand/*.cpp (step 0), built and run -> tests/hand/<name>.out
//! - tests:    examples/confcheck.c.c and tests/test/*.c.c, built with CC_TEST and run -> tests/test/expected/<name>.out
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
        .args(["-std=c++23", "-Wall", "-Wno-type-limits"])
        .args(ccc::cxx::SANITIZE_FLAGS)
        .arg(format!("-I{}", r.join("runtime").display()))
        .arg(format!("-I{}", r.join("third_party").display()))
        .args(extra)
        .arg(cpp)
        .arg("-o")
        .arg(exe)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{}: C++ のビルドに失敗\n{}", cpp.display(), String::from_utf8_lossy(&out.stderr)))
    }
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

#[test]
fn snapshot() {
    let inputs = [files("examples", ".c.c"), files("tests/snapshot", ".c.c")].concat();
    each(inputs, |p| {
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
    let inputs = [vec![root().join("examples/confcheck.c.c")], files("tests/test", ".c.c")].concat();
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

#[test]
fn hand() {
    let dir = tmp_dir("hand");
    each(files("examples/hand", ".cpp"), |p| {
        let exe = dir.join(stem(p));
        cxx_build(p, &exe, &[])?;
        expect(&root().join("tests/hand").join(format!("{}.out", stem(p))), &run_exe(&exe))
    });
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
    let main = |body: &str| format!("fn main() -> i32 effects(io) {{ {body} return 0; }}\n");
    let cases = [
        ("undefined", main("print(nope);"), "'nope'".to_string()),
        ("mismatch", main("let x: i64 = \"a\";"), "cannot convert".to_string()),
        ("narrow_let", main("let n: i64 = 73616; let q: u16 = n;"), "may change value".to_string()),
        ("narrow_literal", main("let q: u16 = 73616;"), "changes value".to_string()),
        ("to_unsigned", main("let n: i64 = -1; let q: u64 = n;"), "may change the sign".to_string()),
        ("sign_compare", main("let a: i64 = -1; let b: u64 = 1; print(a < b);"), "comparison".to_string()),
        ("float", main("let d: f64 = 1.5; let n: i64 = d;"), "may change value".to_string()),
        (
            "no_return",
            "// 2 行目の関数\nfn f(a: i64) -> i64 effects() {\n  if a > 0 {\n    return 1;\n  }\n}\nfn main() -> i32 { return 0; }\n"
                .to_string(),
            "no_return.c.c:2: エラー: control reaches end".to_string(),
        ),
        (
            "literal",
            format!("type Port = u16 where 1 <= self;\n{}", main("let p: Port = 0;")),
            "リテラル 0 は Port の条件を満たしていません".to_string(),
        ),
        (
            "float_try_from",
            format!("type Port = u16 where 1 <= self;\n{}", main("let d: f64 = 8080.5; print(Port::try_from(d));")),
            "deleted".to_string(),
        ),
        (
            "unknown_type",
            "fn f(a: Foo) effects() { }\nfn main() -> i32 { return 0; }\n".to_string(),
            "'Foo'".to_string(),
        ),
    ];
    let errors: Vec<String> = cases
        .iter()
        .filter_map(|(name, source, needle)| {
            let (code, err) = ccc(name, "check", source);
            let hidden = ["note:", "ccc-run-", "^~"].iter().any(|h| err.contains(h));
            let located = err.contains(&format!("{name}.c.c:"));
            (code != Some(1) || !err.contains(needle.as_str()) || hidden || !located)
                .then(|| format!("{name}: exit {code:?}, `{needle}` を期待\n{err}"))
        })
        .collect();
    assert!(errors.is_empty(), "\n{}", errors.join("\n"));
}

#[test]
fn check_accepts_a_valid_program() {
    let (code, err) = ccc("valid", "check", "fn main() -> i32 effects(io) { print(1); return 0; }\n");
    assert_eq!(code, Some(0), "{err}");
}

/// A signal other than the abort of a contract violation must not end the program silently.
#[test]
fn run_reports_death_by_signal() {
    let source = "cpp {\n#include <csignal>\n}\nfn main() -> i32 { cpp { std::raise(SIGSEGV); } return 0; }\n";
    let (code, err) = ccc("signal", "run", source);
    assert_eq!(code, Some(139), "{err}");
    assert!(err.contains("シグナル 11"), "{err}");
}

/// Step 0 completion criterion: `Port::of<0>()` does not compile.
#[test]
fn hand_literal_is_checked_at_compile_time() {
    let dir = tmp_dir("hand-fail");
    let r = cxx_build(&root().join("examples/hand/port.cpp"), &dir.join("port"), &["-DCC_EXPECT_COMPILE_ERROR"]);
    let err = r.expect_err("Port::of<0>() がコンパイルできてしまいました");
    assert!(err.contains("of<0>"), "{err}");
}
