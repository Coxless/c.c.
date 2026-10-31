use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "使い方:
  ccc build <file.c.c> [-o <out.cpp>]   C++ に変換する
  ccc check <file.c.c>                  検査だけ行う
  ccc run   <file.c.c> [-- <args>...]   変換・ビルドして実行する
  ccc test  <file.c.c>                  変換・ビルドして test ブロックを実行する

環境変数:
  CXX       C++ コンパイラ（既定: g++）
  CC_HOME   runtime/ と third_party/ のあるディレクトリ（既定: このリポジトリ）";

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
        "check" => Ok(ExitCode::SUCCESS),
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
            let dir = std::env::temp_dir().join(format!("ccc-run-{}", std::process::id()));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let stem = input.file_name().unwrap().to_string_lossy().trim_end_matches(".c.c").to_string();
            let cpp_path = dir.join(format!("{stem}.cpp"));
            let exe = dir.join(&stem);
            std::fs::write(&cpp_path, cpp).map_err(|e| e.to_string())?;
            let status = Command::new(cxx())
                .args(cxx_flags())
                .args(testing.then_some("-DCC_TEST"))
                .arg(&cpp_path)
                .arg("-o")
                .arg(&exe)
                .status()
                .map_err(|e| format!("{} を起動できません: {e}", cxx()))?;
            if !status.success() {
                return Err("C++ のビルドに失敗しました".into());
            }
            let rest: Vec<&String> = args.iter().skip(2).skip_while(|a| *a == "--").collect();
            let status = Command::new(&exe).args(rest).status().map_err(|e| e.to_string())?;
            let _ = std::fs::remove_dir_all(&dir);
            Ok(ExitCode::from(status.code().unwrap_or(134) as u8))
        }
        _ => Err(USAGE.into()),
    }
}

fn default_output(input: &Path) -> PathBuf {
    let name = input.file_name().unwrap().to_string_lossy();
    input.with_file_name(format!("{}.cpp", name.trim_end_matches(".c.c")))
}

fn cxx() -> String {
    std::env::var("CXX").unwrap_or_else(|_| "g++".into())
}

fn cxx_flags() -> Vec<String> {
    let home = std::env::var("CC_HOME").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/..").into());
    vec!["-std=c++23".into(), format!("-I{home}/runtime"), format!("-I{home}/third_party")]
}
