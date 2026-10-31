use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

use ccc::effects::{self, UserFile};
use ccc::rewrite::{self, Mode};

const USAGE: &str = "使い方:
  ccc build <file.c.c> [-o <out.cpp>]   C++ に変換する（.c.h は出力先に .h として書き出す）
  ccc check <file.c.c>                  検査だけ行う（C++ コンパイラの検査と、Clang による効果の検査）
  ccc run   <file.c.c> [-- <args>...]   変換・ビルドして実行する
  ccc test  <file.c.c>                  変換・ビルドして test ブロックを実行する

環境変数:
  CXX         C++ コンパイラ（既定: g++）
  CC_CLANG    効果の検査に使う Clang（既定: clang++-20、clang++-19、clang++ の順に探す）
  CC_HOME     runtime/ と third_party/ のあるディレクトリ（既定: このリポジトリ）
  CC_VERBOSE  設定すると C++ コンパイラの出力を省略せずに表示する";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(msg) => {
            eprint!("{msg}");
            if !msg.ends_with('\n') {
                eprintln!();
            }
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let (cmd, input) = match args {
        [cmd, input, ..] => (cmd.as_str(), Path::new(input)),
        _ => return Err(USAGE.into()),
    };
    match cmd {
        "check" => check(input),
        "build" => {
            let out = match args.get(2).map(String::as_str) {
                Some("-o") => PathBuf::from(args.get(3).ok_or(USAGE)?),
                None => default_output(input),
                Some(_) => return Err(USAGE.into()),
            };
            let tree = Tree::load(input, Mode::Build)?;
            let dir = out.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
            tree.write_headers(dir, true)?;
            write(&out, &tree.main.cpp)?;
            Ok(ExitCode::SUCCESS)
        }
        "run" | "test" => {
            let testing = cmd == "test";
            let tree = Tree::load(input, Mode::Build)?;
            if testing && !tree.main.has_tests {
                return Err(format!("{}: test ブロックがありません", input.display()));
            }
            let work = WorkDir::create()?;
            let exe = work.0.join(stem(input));
            let mut flags: Vec<&str> = ccc::cxx::SANITIZE_FLAGS.to_vec();
            flags.extend(testing.then_some("-DCC_TEST"));
            let exe_arg = exe.display().to_string();
            flags.extend(["-o", exe_arg.as_str()]);
            compile_cpp(&work, input, &tree, &flags)?;
            let rest: Vec<&String> = args.iter().skip(2).skip_while(|a| *a == "--").collect();
            let status = Command::new(&exe).args(rest).status().map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit_code(status)))
        }
        _ => Err(USAGE.into()),
    }
}

/// `ccc check`: first what any C++ compiler can find, then effects.
fn check(input: &Path) -> Result<ExitCode, String> {
    let tree = Tree::load(input, Mode::Build)?;
    let work = WorkDir::create()?;
    // With CC_TEST the test blocks are compiled as well.
    // A real compile rather than -fsyntax-only, which misses a missing `return`.
    let object = work.0.join("check.o").display().to_string();
    compile_cpp(&work, input, &tree, &["-c", "-DCC_TEST", "-o", object.as_str()])?;

    if !tree.uses_effects() {
        return Ok(ExitCode::SUCCESS);
    }
    let Some(clang) = find_clang() else {
        eprintln!(
            "警告: Clang が見つからないので効果の検査を飛ばします（clang 19 以降が必要です。CC_CLANG で指定できます）"
        );
        return Ok(ExitCode::SUCCESS);
    };
    let tree = Tree::load(input, Mode::Check)?;
    let dir = work.0.join("effects");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let main_path = dir.join(format!("{}.cpp", stem(input)));
    write(&main_path, &tree.main.cpp)?;
    tree.write_headers(&dir, false)?;
    let mut files = vec![UserFile {
        path: main_path.display().to_string(),
        display: file_name(input),
        source: tree.text.clone(),
        cpp: tree.main.cpp.clone(),
    }];
    for h in &tree.headers {
        files.push(UserFile {
            path: dir.join(&h.out_name).display().to_string(),
            display: file_name(&h.source_path),
            source: h.text.clone(),
            cpp: h.output.cpp.clone(),
        });
    }
    let errors = effects::check(&clang, &compiler_paths(&tree), &files)?;
    if errors.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    Err(errors.join("\n"))
}

/// A .c.c file and every .c.h it reaches.
struct Tree {
    main: rewrite::Output,
    text: String,
    source_dir: PathBuf,
    headers: Vec<Header>,
}

struct Header {
    source_path: PathBuf,
    out_name: String,
    text: String,
    output: rewrite::Output,
}

impl Tree {
    fn load(input: &Path, mode: Mode) -> Result<Tree, String> {
        let text = read(input)?;
        let main = rewrite::transform(&input.display().to_string(), &text, mode, false)?;
        let source_dir = parent_dir(input);
        let mut headers: Vec<Header> = Vec::new();
        let mut names: HashMap<String, PathBuf> = HashMap::new();
        // Generated on every run, so nothing can be stale.
        let mut pending: Vec<PathBuf> = main.includes.iter().map(|i| source_dir.join(i)).collect();
        while let Some(path) = pending.pop() {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            let out_name = format!("{}.h", file_name(&path).trim_end_matches(".c.h"));
            match names.get(&out_name) {
                Some(p) if *p == canonical => continue,
                Some(p) => {
                    return Err(format!(
                        "{} と {} は変換後の名前 {out_name} が同じになるので、一緒には使えません",
                        p.display(),
                        canonical.display()
                    ));
                }
                None => {
                    names.insert(out_name.clone(), canonical);
                }
            }
            let text = read(&path)?;
            let output = rewrite::transform(&path.display().to_string(), &text, mode, true)?;
            let dir = parent_dir(&path);
            pending.extend(output.includes.iter().map(|i| dir.join(i)));
            headers.push(Header { source_path: path, out_name, text, output });
        }
        Ok(Tree { main, text, source_dir, headers })
    }

    fn uses_effects(&self) -> bool {
        self.main.uses_effects || self.headers.iter().any(|h| h.output.uses_effects)
    }

    /// With `guard`, refuses to overwrite a file that `ccc` did not generate.
    fn write_headers(&self, dir: &Path, guard: bool) -> Result<(), String> {
        for h in &self.headers {
            let path = dir.join(&h.out_name);
            if guard {
                if let Ok(existing) = std::fs::read_to_string(&path) {
                    if !existing.starts_with("// Generated by ccc from ") {
                        return Err(format!(
                            "{}: ccc が生成したファイルではないので上書きしません（{} の変換先です）",
                            path.display(),
                            h.source_path.display()
                        ));
                    }
                }
            }
            write(&path, &h.output.cpp)?;
        }
        Ok(())
    }

    /// Plain `#include "x.h"` has to keep working although the C++ is compiled elsewhere.
    fn quote_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.source_dir.clone()];
        for h in &self.headers {
            let d = parent_dir(&h.source_path);
            if !dirs.contains(&d) {
                dirs.push(d);
            }
        }
        dirs
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: 読み込めません: {e}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|e| format!("{}: 書き込めません: {e}", path.display()))
}

fn parent_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// A temporary directory, removed on every way out of `run`.
struct WorkDir(PathBuf);

impl WorkDir {
    fn create() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("ccc-run-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(WorkDir(dir))
    }
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the C++ compiler on the tree, reporting failures in terms of the .c.c file.
fn compile_cpp(work: &WorkDir, input: &Path, tree: &Tree, flags: &[&str]) -> Result<(), String> {
    let cpp_path = work.0.join(format!("{}.cpp", stem(input)));
    write(&cpp_path, &tree.main.cpp)?;
    tree.write_headers(&work.0, false)?;
    let out = Command::new(cxx())
        .args(["-std=c++23"])
        .args(compiler_paths(tree))
        .args(flags)
        .arg("-fdiagnostics-color=never")
        .arg(&cpp_path)
        .env("LC_ALL", "C") // `summarize` reads the output, so it must not be translated.
        .output()
        .map_err(|e| format!("{} を起動できません: {e}", cxx()))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let shown = if std::env::var_os("CC_VERBOSE").is_some() {
        stderr.to_string()
    } else {
        ccc::cxx::summarize(&stderr, &file_name(input), &tree.text)
    };
    Err(format!("{shown}C++ のビルドに失敗しました（全文は CC_VERBOSE=1 で表示できます）"))
}

/// The conventional shell code. A contract violation has reported itself before
/// aborting; any other death by signal would otherwise be silent.
fn exit_code(status: ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return code as u8;
    }
    let signal = signal_of(status);
    if signal != 6 {
        eprintln!("プログラムが異常終了しました（シグナル {signal}）");
    }
    (128 + signal) as u8
}

#[cfg(unix)]
fn signal_of(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.signal().unwrap_or(6)
}

#[cfg(not(unix))]
fn signal_of(_: ExitStatus) -> i32 {
    6
}

fn stem(input: &Path) -> String {
    file_name(input).trim_end_matches(".c.c").trim_end_matches(".cpp").to_string()
}

fn default_output(input: &Path) -> PathBuf {
    input.with_file_name(format!("{}.cpp", stem(input)))
}

fn cxx() -> String {
    std::env::var("CXX").unwrap_or_else(|_| "g++".into())
}

fn find_clang() -> Option<String> {
    let usable = |c: &String| Command::new(c).arg("--version").output().is_ok_and(|o| o.status.success());
    std::env::var("CC_CLANG")
        .ok()
        .into_iter()
        .chain(["clang++-22", "clang++-21", "clang++-20", "clang++-19", "clang++"].iter().map(|s| s.to_string()))
        .find(usable)
}

fn compiler_paths(tree: &Tree) -> Vec<String> {
    let home = std::env::var("CC_HOME").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/..").into());
    let mut flags = vec![format!("-I{home}/runtime"), format!("-isystem{home}/third_party")];
    for d in tree.quote_dirs() {
        flags.push(format!("-iquote{}", d.display()));
    }
    flags
}
