use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

const USAGE: &str = "使い方:
  ccc build <file.c.c> [-o <out.cpp>]   C++ に変換する
  ccc check <file.c.c>                  検査だけ行う（C++ コンパイラの検査も含む。実行ファイルは作らない）
  ccc run   <file.c.c> [-- <args>...]   変換・ビルドして実行する
  ccc test  <file.c.c>                  変換・ビルドして test ブロックを実行する

環境変数:
  CXX         C++ コンパイラ（既定: g++）
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
    let text = std::fs::read_to_string(input).map_err(|e| format!("{}: 読み込めません: {e}", input.display()))?;
    let cpp = ccc::compile(&input.display().to_string(), &text)?;
    match cmd {
        "check" => {
            let work = WorkDir::create()?;
            // With CC_TEST the test blocks are compiled as well; only the small `main` wrapper is not.
            // A real compile rather than -fsyntax-only, which misses a missing `return`.
            let object = work.0.join("check.o").display().to_string();
            compile_cpp(&work, input, &text, cpp, &["-c", "-DCC_TEST", "-o", object.as_str()])?;
            Ok(ExitCode::SUCCESS)
        }
        "build" => {
            let out = match args.get(2).map(String::as_str) {
                Some("-o") => PathBuf::from(args.get(3).ok_or(USAGE)?),
                None => default_output(input),
                Some(_) => return Err(USAGE.into()),
            };
            std::fs::write(&out, cpp).map_err(|e| format!("{}: 書き込めません: {e}", out.display()))?;
            Ok(ExitCode::SUCCESS)
        }
        "run" | "test" => {
            let testing = cmd == "test";
            if testing && !cpp.contains("#ifdef CC_TEST") {
                return Err(format!("{}: test ブロックがありません", input.display()));
            }
            let work = WorkDir::create()?;
            let exe = work.0.join(stem(input));
            let mut flags: Vec<&str> = ccc::cxx::SANITIZE_FLAGS.to_vec();
            flags.extend(testing.then_some("-DCC_TEST"));
            let exe_arg = exe.display().to_string();
            flags.extend(["-o", exe_arg.as_str()]);
            compile_cpp(&work, input, &text, cpp, &flags)?;
            let rest: Vec<&String> = args.iter().skip(2).skip_while(|a| *a == "--").collect();
            let status = Command::new(&exe).args(rest).status().map_err(|e| e.to_string())?;
            Ok(ExitCode::from(exit_code(status)))
        }
        _ => Err(USAGE.into()),
    }
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

/// Runs the C++ compiler on `cpp`, reporting failures in terms of the .c.c file.
fn compile_cpp(work: &WorkDir, input: &Path, text: &str, cpp: String, flags: &[&str]) -> Result<(), String> {
    let cpp_path = work.0.join(format!("{}.cpp", stem(input)));
    std::fs::write(&cpp_path, cpp).map_err(|e| e.to_string())?;
    let out = Command::new(cxx())
        .args(cxx_flags())
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
        let file = input.file_name().unwrap().to_string_lossy();
        ccc::cxx::summarize(&stderr, &file, text)
    };
    Err(format!(
        "{shown}C++ のビルドに失敗しました（型の誤りの多くは C++ コンパイラが検出しています。全文は CC_VERBOSE=1 で表示できます）"
    ))
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
    input.file_name().unwrap().to_string_lossy().trim_end_matches(".c.c").to_string()
}

fn default_output(input: &Path) -> PathBuf {
    input.with_file_name(format!("{}.cpp", stem(input)))
}

fn cxx() -> String {
    std::env::var("CXX").unwrap_or_else(|_| "g++".into())
}

fn cxx_flags() -> Vec<String> {
    let home = std::env::var("CC_HOME").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/..").into());
    vec!["-std=c++23".into(), format!("-I{home}/runtime"), format!("-I{home}/third_party")]
}
