//! Golden tests over files in the repository:
//!
//! - snapshot: examples/*.c.c and tests/snapshot/*.c.c -> tests/snapshot/<name>.cpp
//! - run:      examples/*.c.c and tests/run/*.c.c, built with g++ and run -> tests/run/expected/<name>.out
//! - errors:   tests/errors/*.c.c -> tests/errors/<name>.err
//! - hand:     examples/hand/*.cpp (step 0), built and run -> tests/hand/<name>.out
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
    let out = Command::new(exe).output().unwrap();
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

/// Step 0 completion criterion: `Port::of<0>()` does not compile.
#[test]
fn hand_literal_is_checked_at_compile_time() {
    let dir = tmp_dir("hand-fail");
    let r = cxx_build(&root().join("examples/hand/port.cpp"), &dir.join("port"), &["-DCC_EXPECT_COMPILE_ERROR"]);
    let err = r.expect_err("Port::of<0>() がコンパイルできてしまいました");
    assert!(err.contains("of<0>"), "{err}");
}
