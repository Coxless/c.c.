# C.C.（Contract-Centric C++）

[![CI](https://github.com/Coxless/c.c./actions/workflows/ci.yml/badge.svg)](https://github.com/Coxless/c.c./actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**C++ に対する TypeScript のような言語です。** 普通の C++ はそのまま通ります。そこに値・関数・効果の「契約」を足すと、その部分から検査が効き始めます。`.c.c` ファイルをトランスパイラ `ccc` で C++23 に変換し、普通の C++ コンパイラでビルドします。

> [!WARNING]
> **設計を試すための試作です。** 構文は変わる可能性があり、実用には向きません。動作を確認しているのは Ubuntu 24.04、g++ 13、clang 19 / 20 だけです。

`ccc` が変えるのは拡張の部分だけで、それ以外の C++ は1文字も変わりません。足す構文は `type`、`schema`、`pre` / `post`、`test` だけです。

## 例

[examples/tour.c.c](examples/tour.c.c):

```cpp
#include <cc/cc.hpp>

#include <iostream>
#include <string>

type Port = u16 where 1 <= self <= 65535;

schema Server {
  name:    string where 1 <= len(self) <= 50 transform trim;
  port:    Port;
  workers: i64    where 1 <= self <= 256 = 4;
}

std::int64_t per_worker(std::int64_t requests, std::int64_t workers)
  pre(workers > 0, "ワーカーが1つ以上ある")
  post(r: r * workers <= requests, "割り当ての合計が総数を超えない")
{
  return requests / workers;
}

void load(const std::string& text) {
  if (auto s = Server::parse(cc::parse_json(text))) {
    std::cout << s->name << " port " << s->port << " workers " << s->workers << "\n";
  } else {
    std::cout << s.error().to_string() << "\n";
  }
}

int main() {
  Port admin = 8443;
  std::cout << "admin port " << admin << "\n";
  load(R"({"name": " edge ", "port": 8080})");
  load(R"({"name": "", "port": 70000, "workers": 0, "wokers": 8})");
  std::cout << per_worker(100, 3) << std::endl;
  std::cout << per_worker(100, 0) << std::endl;
}
```

実行結果:

```
admin port 8443
edge port 8080 workers 4
Server の検証に失敗しました（4 件）
  - name: 1 <= len(self) <= 50 を満たしていません
  - port: 範囲内の整数が必要です
  - workers: 1 <= self <= 256 を満たしていません
  - wokers: 未知のフィールドです
33
contract violation: pre「ワーカーが1つ以上ある」
  条件: workers > 0
  責任: 呼び出し側  tour.c.c:36  main() → per_worker()
  契約: tour.c.c:16
  値:   workers = 0
```

- `schema` 1つから、構造体、検証付きの `parse()` と `make()`、`to_json()` が生成されます
- 検証エラーは最初の1件で止まらず、全件報告されます
- 契約違反は、呼び出し側と実装側のどちらの責任かを `.c.c` の行番号付きで報告します

次の誤りは、実行する前に見つかります。

```
bad.c.c:8: エラー: リテラル 0 は Port の条件を満たしていません
  |
8 |   Port p = 0;
```

効果は `[[cc::effects(...)]]` で宣言します。`ccc check` が Clang の解析結果を読むので、ソースに見えない呼び出しも検査できます。

```cpp
struct Widget {
  [[cc::effects(io)]] ~Widget();
};

[[cc::effects()]] void f() {
  Widget w;
}
```

```
a.c.c:6:3: エラー: デストラクタ `Widget::~Widget`（スコープの終わりなどで暗黙に呼ばれます）には効果 io が必要ですが、`f` は effects() しか宣言していません
  |
6 |   Widget w;
  |   ^
```

## TypeScript との対応

| TypeScript | C.C. |
|---|---|
| JS はほぼそのまま TS として通る | `.cpp` を `.c.c` に改名すれば通る |
| 型注釈を足すほど検査が効く | `schema`、`pre` / `post`、`effects` を足すほど検査が効く |
| `any` | 注釈のない関数、`unsafe` |
| `strict` | `#pragma cc strict` |
| `declare`、`.d.ts` | 既存の関数への `[[cc::effects(...)]]` |

## できること／まだできないこと

| 機能 | 状況 |
|---|---|
| 既存の C++ をそのまま通す | ✅ |
| 条件付きの型（`where`、`matches`）。`Port p = 8080;` はコンパイル時に検査 | ✅ |
| `schema` からの構造体・`parse()`・`make()`・`to_json()` の生成、エラーの全件報告 | ✅ |
| `pre` / `post` / `old()` と、違反の責任の報告 | ⚠️ 普通の関数だけ。`template` とメンバ関数はエラーになる |
| `Raw<T>` による未検証データと検証済みデータの区別 | ✅ |
| Clang による効果の検査（暗黙の呼び出しを含む） | ✅ `ccc check`。clang 19 以降が必要 |
| `test` ブロックと、`cc::read_file` の差し替え | ✅ |
| 厳格モード（値が変わる暗黙の変換、`return` のない経路、`v[i]` の範囲外） | ✅ `#pragma cc strict` |
| 整数オーバーフローとゼロ除算の検出 | ⚠️ `ccc run` / `ccc test` のときだけ |
| 拡張付きヘッダー（`.c.h`） | ✅ |
| Z3 による静的証明、実行時チェックの削減 | ❌ 未着手 |
| スキーマからのテスト自動生成、JSON Schema の出力 | ❌ 未着手 |
| 所有（`once`、`fixed`）と手順（`protocol`） | ❌ 未着手 |

すべての制限は [PLAN.md の既知の制限](PLAN.md#4-既知の制限)にあります。

## 始め方

必要なもの:

- Rust（cargo）
- g++ 13 以降と libubsan（Ubuntu 24.04 では `g++` と一緒に入ります）
- 効果の検査を使うなら clang 19 以降（なければ検査を飛ばして警告を出します）

```sh
git clone https://github.com/Coxless/c.c..git
cd c.c.
cargo build --manifest-path compiler/Cargo.toml
compiler/target/debug/ccc run examples/tour.c.c
```

ホストに何も入れたくない場合は、[Ubuntu Workshop](https://ubuntu.com/workshop) のコンテナの中で使えます。定義は `.workshop/dev.yaml` にあり、プロジェクトは `/project` にマウントされます。

```sh
workshop launch            # 初回のみ。cargo、g++、Clang を入れる
workshop run -- test       # cargo test
workshop shell             # 以降のコマンドはこの中で実行する（cd /project）
```

## 使い方

```sh
ccc run   file.c.c [-- 引数...]    # 変換・ビルドして実行する
ccc test  file.c.c                 # test ブロックを実行する
ccc check file.c.c                 # 検査だけ行う（C++ コンパイラの検査と、Clang による効果の検査）
ccc build file.c.c [-o out.cpp]    # C++ に変換する（.c.h は出力先に .h として書き出す）
```

| 環境変数 | 内容 |
|---|---|
| `CXX` | C++ コンパイラ（既定: `g++`） |
| `CC_CLANG` | 効果の検査に使う Clang（既定: `clang++-22` 〜 `clang++-19`、`clang++` の順に探す） |
| `CC_HOME` | `runtime/` と `third_party/` のあるディレクトリ（既定: ビルドしたときのリポジトリ） |
| `CC_VERBOSE` | 設定すると C++ コンパイラの出力を省略せずに表示する |

変換した C++ を自分でビルドするときは、`runtime/` と `third_party/` をインクルードパスに加えます。

```sh
g++ -std=c++23 -Iruntime -isystem third_party out.cpp -o out
```

`ccc run` と `ccc test` は、UBSan を有効にしてビルドします（`-fsanitize=undefined -fno-sanitize-recover=undefined`）。自分でビルドするときも、オーバーフローとゼロ除算を検出したければこのオプションを加えてください。

## サンプル

| ファイル | 内容 |
|---|---|
| [examples/tour.c.c](examples/tour.c.c) | 上の例 |
| [examples/confcheck.c.c](examples/confcheck.c.c) | 設定ファイルを検証する CLI。厳格モード、効果、`enum`、`test` を使った実用例 |
| [examples/port.c.c](examples/port.c.c) | 条件付きの型 |
| [examples/withdraw.c.c](examples/withdraw.c.c) | `pre` / `post` / `old()` と違反の責任の報告 |
| [examples/user.c.c](examples/user.c.c) | `schema` の生成物とエラーの全件報告 |
| [examples/effects.c.c](examples/effects.c.c) | 効果の宣言と、テストでの差し替え |
| [examples/hand/](examples/hand/) | `ccc` を使わず、`cc.hpp` をライブラリとして使った普通の C++ |

## ドキュメント

| ファイル | 内容 |
|---|---|
| [docs/language.md](docs/language.md) | 言語仕様（今の `ccc` が受け付ける言語の定義と、つまずきやすい点） |
| [CONCEPT.md](CONCEPT.md) | 言語の考え方と、将来の構想 |
| [PLAN.md](PLAN.md) | 実装の状況、決めたこと、既知の制限 |

## 開発

```sh
cd compiler
cargo test            # 互換性・スナップショット・実行・エラー・効果の検査のテスト
UPDATE=1 cargo test   # 期待値ファイルを更新する
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

CI では、Ubuntu 24.04 の上でこの `cargo fmt`、`cargo clippy`、`cargo test` を実行します。

| ディレクトリ | 内容 |
|---|---|
| `compiler/` | Rust 製トランスパイラ `ccc`。依存クレートなし |
| `compiler/src/scan.rs`、`rewrite.rs` | C++ の字句を読み、拡張だけを置き換える |
| `compiler/src/lexer.rs`、`parser.rs`、`check.rs`、`emit.rs` | `type` と `schema` の解析、検査、生成 |
| `compiler/src/effects.rs`、`effect_table.rs` | Clang の AST を読む効果の検査と、標準ライブラリの効果の一覧 |
| `runtime/cc/` | ライブラリ。`cc.hpp` と、効果の注釈だけの `effects.hpp` |
| `third_party/` | nlohmann/json |
| `tests/compat/` | 普通の C++ が変わらず動くこと |
| `tests/snapshot/` | `.c.c` → `.cpp` の出力 |
| `tests/run/` | 生成 C++ をビルド・実行した結果（`expected/*.out`） |
| `tests/errors/` | 不正な `.c.c` に対するエラーメッセージ |
| `tests/test/` | `test` ブロックの実行結果（`expected/*.out`） |
| `tests/effects/` | 効果の検査 |
| `tests/headers/` | `.c.h` |

## ライセンス

[MIT](LICENSE)

`third_party/` のコードには、それぞれのライセンスが適用されます。生成したプログラムは nlohmann/json を取り込むので、配布するときは [third_party/README.md](third_party/README.md) のライセンス表示を同梱してください。
