# C.C.（Contract-Centric C++）

[![CI](https://github.com/Coxless/c.c./actions/workflows/ci.yml/badge.svg)](https://github.com/Coxless/c.c./actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

値・関数・効果の「契約」を中心にした、C++ へ変換する言語です。`.c.c` ファイルをトランスパイラ `ccc` で C++23 に変換し、普通の C++ コンパイラでビルドします。

> [!WARNING]
> **設計を試すための試作です。** 構文は変わる可能性があり、実用には向きません。動作を確認しているのは Ubuntu 24.04 と g++ 13 だけです。

目標は「一度書いた契約が、型・検証・テスト・ドキュメントのすべてになる」言語です。現時点でできる範囲は[できること／まだできないこと](#できることまだできないこと)にまとめています。

## 例

[examples/tour.c.c](examples/tour.c.c):

```cpp
type Port = u16 where 1 <= self <= 65535;

schema Server {
  name:    string where 1 <= len(self) <= 50 transform trim;
  port:    Port;
  workers: i64    where 1 <= self <= 256 = 4;
}

contract fn per_worker(requests: i64, workers: i64) -> i64
  effects()
  pre(workers > 0, "ワーカーが1つ以上ある")
  post(r: r * workers <= requests, "割り当ての合計が総数を超えない")
{
  return requests / workers;
}

fn load(text: string) effects(io) {
  match Server::parse(parse_json(text)) {
    ok(s)  { print(s.name, "port", s.port, "workers", s.workers); }
    err(e) { print(e); }
  }
}

fn main() -> i32 effects(io) {
  load(R"({"name": " edge ", "port": 8080})");
  load(R"({"name": "", "port": 70000, "workers": 0, "wokers": 8})");
  print(per_worker(100, 3));
  print(per_worker(100, 0));
  return 0;
}
```

実行結果:

```
edge port 8080 workers 4
Server の検証に失敗しました（4 件）
  - name: 1 <= len(self) <= 50 を満たしていません
  - port: 範囲内の整数が必要です
  - workers: 1 <= self <= 256 を満たしていません
  - wokers: 未知のフィールドです
33
contract violation: pre「ワーカーが1つ以上ある」
  条件: workers > 0
  責任: 呼び出し側  tour.c.c:32  main() → per_worker()
  契約: tour.c.c:15
  値:   workers = 0
```

- `schema` 1つから、構造体、検証付きの `parse()`、`to_json()` が生成されます
- 検証エラーは最初の1件で止まらず、全件報告されます
- 契約違反は、呼び出し側と実装側のどちらの責任かを `.c.c` の行番号付きで報告します

次の誤りは、実行する前に見つかります。

```
bad.c.c:2:3: エラー: `print` の呼び出しには効果 io が必要ですが、`add` は effects() しか宣言していません
  |
2 |   print("add");
  |   ^^^^^^^^^^^^
```

```
raw.c.c:5:9: エラー: 未検証の Raw<Json> を User として使えません
  |
5 |   greet(body);
  |         ^^^^
  = 補足: 先に User::parse(...) で検証してください（ここが信頼境界です）
```

## できること／まだできないこと

| 機能 | 状況 |
|---|---|
| 条件付きの型（`where`、`matches`）。リテラルはコンパイル時に検査 | ✅ |
| `schema` からの構造体・`parse()`・`to_json()` の生成、エラーの全件報告 | ✅ |
| `pre` / `post` / `old()` と、違反の責任の報告 | ✅ |
| 効果（`effects(...)`）の検査 | ✅ |
| `Raw<T>` による未検証データと検証済みデータの区別 | ✅ |
| `test` ブロックと、`with` によるファイル読み込みの差し替え | ✅ |
| `v[i]` の範囲検査。値が変わる暗黙の変換の禁止 | ✅ |
| 整数オーバーフローとゼロ除算の検出 | ⚠️ `ccc run` / `ccc test` のときだけ |
| 型検査 | ⚠️ 多くを C++ コンパイラに任せているので、エラーの文面は C++ のもの |
| Z3 による静的証明、実行時チェックの削減 | ❌ 未着手 |
| スキーマからのテスト自動生成、JSON Schema の出力 | ❌ 未着手 |
| 所有（`once`、`fixed`）と手順（`protocol`） | ❌ 未着手 |

試作として割り切っている点もあります。

- 構造体の値を書く構文がありません。スキーマ型の値は `parse` で作るか、`cpp { }` か `extern fn` に頼ります
- `vec` への追加など、C++ のメソッド呼び出しには `unsafe` が必要です
- 1ファイル単位です。モジュール、ジェネリクス、オーバーロードはありません
- `extern fn` の効果の宣言は信用するだけで、検証はしません

すべての制限は [PLAN.md の既知の制限](PLAN.md#既知の制限区切りでの見直し候補)にあります。

## 始め方

必要なもの:

- Rust（cargo）
- g++ 13 以降と libubsan（Ubuntu 24.04 では `g++` と一緒に入ります）

clang 18 と libstdc++ 13 の組み合わせでは、`std::expected` を使えないので動きません。

```sh
git clone https://github.com/Coxless/c.c..git
cd c.c.
cargo build --manifest-path compiler/Cargo.toml
compiler/target/debug/ccc run examples/tour.c.c
```

ホストに何も入れたくない場合は、[Ubuntu Workshop](https://ubuntu.com/workshop) のコンテナの中で使えます。定義は `.workshop/dev.yaml` にあり、プロジェクトは `/project` にマウントされます。

```sh
workshop launch            # 初回のみ。cargo と g++ を入れる
workshop run -- test       # cargo test
workshop shell             # 以降のコマンドはこの中で実行する（cd /project）
```

## 使い方

```sh
ccc run   file.c.c [-- 引数...]    # 変換・ビルドして実行する
ccc test  file.c.c                 # test ブロックを実行する
ccc check file.c.c                 # 検査だけ行う（C++ コンパイラの検査も含む）
ccc build file.c.c [-o out.cpp]    # C++ に変換する
```

| 環境変数 | 内容 |
|---|---|
| `CXX` | C++ コンパイラ（既定: `g++`） |
| `CC_HOME` | `runtime/` と `third_party/` のあるディレクトリ（既定: ビルドしたときのリポジトリ） |
| `CC_VERBOSE` | 設定すると C++ コンパイラの出力を省略せずに表示する |

変換した C++ を自分でビルドするときは、`runtime/` と `third_party/` をインクルードパスに加えます。

```sh
g++ -std=c++23 -Iruntime -Ithird_party out.cpp -o out
```

`ccc run` と `ccc test` は、UBSan を有効にしてビルドします（`-fsanitize=undefined -fno-sanitize-recover=undefined`）。自分でビルドするときも、オーバーフローとゼロ除算を検出したければこのオプションを加えてください。

## サンプル

| ファイル | 内容 |
|---|---|
| [examples/tour.c.c](examples/tour.c.c) | 上の例 |
| [examples/confcheck.c.c](examples/confcheck.c.c) | 設定ファイルを検証する CLI。`enum`、`match`、`try`、`test` を使った実用例 |
| [examples/port.c.c](examples/port.c.c) | 条件付きの型 |
| [examples/withdraw.c.c](examples/withdraw.c.c) | `pre` / `post` / `old()` と違反の責任の報告 |
| [examples/user.c.c](examples/user.c.c) | `schema` の生成物とエラーの全件報告 |
| [examples/effects.c.c](examples/effects.c.c) | 効果の宣言と `?` |
| [examples/hand/](examples/hand/) | トランスパイラなしで手書きした「生成コードの完成形」 |

## ドキュメント

| ファイル | 内容 |
|---|---|
| [docs/language.md](docs/language.md) | 実装済みの言語機能のリファレンス |
| [CONCEPT.md](CONCEPT.md) | 言語の考え方と、将来の構想 |
| [PLAN.md](PLAN.md) | 実装計画、進捗、既知の制限 |
| [docs/limitations-memo.md](docs/limitations-memo.md) | 制限ごとの改善策 |
| [docs/implementation.html](docs/implementation.html) | 実装の解説（ダウンロードしてブラウザで開きます） |

## 開発

```sh
cd compiler
cargo test            # スナップショット・実行・エラー・手書き版・test ブロックのテスト
UPDATE=1 cargo test   # 期待値ファイルを更新する
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

CI では、Ubuntu 24.04 の上でこの `cargo fmt`、`cargo clippy`、`cargo test` を実行します。

| ディレクトリ | 内容 |
|---|---|
| `compiler/` | Rust 製トランスパイラ `ccc`（lexer → parser → check → emit）。依存クレートなし |
| `runtime/cc/cc.hpp` | 生成コードが使うヘッダーライブラリ |
| `third_party/` | nlohmann/json |
| `tests/snapshot/` | `.c.c` → `.cpp` の出力 |
| `tests/run/` | 生成 C++ をビルド・実行した結果（`expected/*.out`） |
| `tests/errors/` | 不正な `.c.c` に対するエラーメッセージ |
| `tests/hand/` | `examples/hand/` の実行結果 |
| `tests/test/` | `test` ブロックの実行結果（`expected/*.out`） |

## ライセンス

[MIT](LICENSE)

`third_party/` のコードには、それぞれのライセンスが適用されます。生成したプログラムは nlohmann/json を取り込むので、配布するときは [third_party/README.md](third_party/README.md) のライセンス表示を同梱してください。
