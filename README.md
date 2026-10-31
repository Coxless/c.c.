# C.C.（Contract-Centric C++）

一度書いた契約が、型・検証・テスト・ドキュメントのすべてになる C++ ベースの言語の試作です。

- 言語の考え方: [CONCEPT.md](CONCEPT.md)
- 実装計画と進捗: [PLAN.md](PLAN.md)

`.c.c` ファイルをトランスパイラ `ccc` で C++23 に変換し、普通の C++ コンパイラでビルドします。

## 必要なもの

- Rust（cargo）
- g++ 13 以降（`std::expected` を使うため。clang 18 + libstdc++ 13 の組み合わせでは `std::expected` が使えません）

ホストに入れずに、[Ubuntu Workshop](https://ubuntu.com/workshop) のコンテナの中で使うこともできます。定義は `.workshop/dev.yaml` にあり、プロジェクトは `/project` にマウントされます。

```sh
workshop launch            # 初回のみ。cargo と g++ を入れる
workshop run -- test       # cargo test
workshop shell             # 以下の「使い方」はこの中で実行する（cd /project）
```

## 使い方

```sh
cd compiler
cargo build
./target/debug/ccc run   ../examples/user.c.c           # 変換・ビルドして実行
./target/debug/ccc build ../examples/user.c.c -o user.cpp  # C++ に変換
./target/debug/ccc check ../examples/user.c.c           # 検査だけ
```

生成した C++ を自分でビルドする場合は `runtime/` と `third_party/` をインクルードパスに加えます。

```sh
g++ -std=c++23 -Iruntime -Ithird_party user.cpp -o user
```

## 例

| ファイル | 内容 |
|---|---|
| [examples/port.c.c](examples/port.c.c) | 条件付きの型。リテラルはコンパイル時に検査される |
| [examples/withdraw.c.c](examples/withdraw.c.c) | `pre` / `post` / `old()` と、違反の責任（呼び出し側／実装側）の報告 |
| [examples/user.c.c](examples/user.c.c) | `schema` から struct・`parse()`・`to_json()` を生成。エラーは全件報告 |
| [examples/effects.c.c](examples/effects.c.c) | 効果の宣言と `?` |
| [examples/hand/](examples/hand/) | トランスパイラなしで手書きした「生成コードの完成形」（ステップ0） |

違反レポートの例（`examples/withdraw.c.c`）:

```
contract violation: pre「残高が足りる」
  条件: a.balance >= amt
  責任: 呼び出し側  withdraw.c.c:35  pay() → withdraw()
  契約: withdraw.c.c:11
  値:   a.balance = 200, amt = 500
```

## 実装済みの言語機能

### 宣言

```cpp
type Port  = u16 where 1 <= self <= 65535;     // 条件付きの型（ブランド型）
type Email = string matches email_format;       // 述語関数による条件

schema User {
  id:    u64         where self > 0;
  name:  string      where 1 <= len(self) <= 50 transform trim;
  email: Email;
  age:   int?        where 0 <= self <= 150;    // T? は省略可能
  tags:  vec<string> where len(self) <= 3 = [];  // 既定値
  invariant(age == none || age >= 13, "13歳以上");
}

contract fn withdraw(Account& a, i64 amt) -> i64
  effects(log)
  pre(amt > 0, "金額は正")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{ ... }

fn greet(User u) effects(io) { ... }
extern fn open_account(i64 b) -> Account effects();   // C++ で実装された関数の宣言
cpp { /* 生の C++（逃げ道） */ }
```

- 型: `i32` `i64`（`int`）`u8` `u16` `u32` `u64` `f64` `bool` `string`（`str`）`vec<T>` `T?` `Json` `Raw<T>`
- 文: `let`（`let x: T = ...` も可）、代入（`=` `+=` など）、`return`、`if` / `else`、`for x in xs`、`for i in a..b`、`while`、`cpp { ... }`
- 式: 算術、比較（`1 <= x <= 9` のような連鎖も可）、論理、関数呼び出し、`T::parse` / `T::try_from`、フィールド・添字・メソッド呼び出し、`none`、`[a, b]`、`R"(...)"`
- 組み込み関数: `len` `trim` `email_format` `parse_json`（純粋）、`print`（io）、`log`（log）、`read_file`（io, throw）

### 値の契約

- `Port p = 8080;` に相当する `let p: Port = 8080;` は `Port::of<8080>()` になり、条件はコンパイル時に検査されます
- 静的に分からない値（`n + 1` など）から暗黙には変換できません。`Port::try_from(...)` を使います
- 外部入力は `Raw<Json>` で受け取り、`User::parse(...)` を通したものだけが `User` になります。`Raw<Json>` を `User` として渡すとコンパイルエラーです

### 関数の契約

- `pre` の違反は呼び出し側、`post` の違反は実装側の責任として、`.c.c` の行番号付きで報告します
- 既定では違反で停止します。`-DCC_CONTRACTS_OBSERVE`（または `cc::contract_mode = cc::Mode::observe;`）で報告だけして続行します

### 効果

- 関数が呼ぶものの効果は、その関数が `effects(...)` で宣言した効果に含まれていなければなりません
- `effects(...)` を省略した関数は検査されず、何でもできます（既存コードから段階的に導入するため）
- 効果が分からないもの（`effects` を省略した関数、`extern fn` で宣言していない C++ 関数、関数内の `cpp { }`）を呼ぶには `unsafe` が必要です
- `?` は失敗時に例外を送出するので `throw` が必要です。`throw` を宣言していない関数には `noexcept` が付きます
- `where` / `pre` / `post` / `invariant` の条件と既定値は純粋でなければなりません

## テスト

```sh
cd compiler
cargo test            # スナップショット・実行・エラー・手書き版のテスト
UPDATE=1 cargo test   # 期待値ファイルを更新する
```

| ディレクトリ | 内容 |
|---|---|
| `tests/snapshot/` | `.c.c` → `.cpp` の出力 |
| `tests/run/` | 生成 C++ をビルド・実行した結果（`expected/*.out`） |
| `tests/errors/` | 不正な `.c.c` に対するエラーメッセージ |
| `tests/hand/` | `examples/hand/` の実行結果 |

## 構成

```
runtime/cc/cc.hpp   生成コードが使うヘッダーライブラリ
compiler/           Rust 製トランスパイラ ccc（lexer → parser → check → emit）
third_party/        nlohmann/json（MIT）
```
