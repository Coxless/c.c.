# C.C. 実装計画（小さく始める版）

- 対象: [CONCEPT.md](CONCEPT.md) ドラフト 0.2
- 方針: **動くものを最短で作り、1ステップごとに「.c.c を書く → C++ が出る → 実行できる」を確認する**

---

## 1. 最初に決めること（未決事項のうち、試作に必要な分だけ）

| 項目 | 試作での決定 | 理由 |
|---|---|---|
| トランスパイラの実装言語 | **Rust** | enum とパターンマッチで AST・型検査が書きやすい。この環境に cargo 1.97 がある |
| 出力する C++ | **C++23**（`std::expected` を使う） | g++ 13 / clang 18 は C++26 Contracts 未対応。`std::expected` は g++ 13 で動作確認済み |
| 契約の実行時チェック | **自前マクロ**（`CC_PRE` / `CC_POST`） | 未決事項6。C++26 Contracts がコンパイラに入ったら差し替える |
| JSON | **nlohmann/json**（単一ヘッダーを `third_party/` に置く） | コンセプトどおり。依存は1ファイルだけ |
| `where` に書ける式 | 算術・比較・論理・`len()`・`self` のみ | 未決事項4。後で Z3 に渡せる範囲に最初から絞る |
| スキーマのエラー | フィールドのパス（`tags[3]`）付きで全件返す | 未決事項3。後から足すと型が変わるので最初から入れる |
| 効果の省略 | フェーズ2まで保留（それまで効果は検査しない） | 最初の数ステップでは不要 |

---

## 2. ディレクトリ構成

```
c.c./
├── CONCEPT.md
├── PLAN.md
├── runtime/            # 生成コードが include するヘッダーライブラリ（ステップ0）
│   └── cc/cc.hpp
├── third_party/
│   └── nlohmann/json.hpp
├── compiler/           # Rust 製トランスパイラ `ccc`（ステップ1〜）
│   ├── Cargo.toml
│   └── src/{main.rs, lexer.rs, parser.rs, ast.rs, check.rs, emit.rs}
├── examples/           # .c.c のサンプルと手書きの期待 C++
└── tests/
    ├── snapshot/       # .c.c → .cpp の出力比較
    └── run/            # 生成した C++ をビルドして実行し、出力を比較
```

---

## 3. ステップ

各ステップは単独で完了・コミットできる大きさにする。「完了条件」を満たしたら次へ進む。

### ステップ0：ヘッダーライブラリだけで動かす（トランスパイラなし）

生成コードの「完成形」を先に手で書き、トランスパイラの出力目標にする。

- `runtime/cc/cc.hpp`
  - `RefineError`、`ParseError`（`{path, condition, message}` のリスト）
  - `CC_PRE(cond, msg)` / `CC_POST(cond, msg)`：違反時に**責任（呼び出し側／実装側）**、条件、ソース位置を表示して `abort`
  - 呼び出し位置は `std::source_location` を既定引数で受け取って記録する
- `examples/hand/port.cpp`：コンセプト 9.4 の `Port` を手書き
- `examples/hand/user.cpp`：`User::parse(json)` がエラーを**全件**返す
- `examples/hand/withdraw.cpp`：`pre` / `post` / `old()` と違反レポート

**完了条件**：`g++ -std=c++23` でビルドでき、次の3つを確認できる
1. `Port::of<8080>()` は通り、`Port::of<0>()` はコンパイルエラー
2. 不正な JSON を `User::parse` に渡すと、壊れたフィールドがすべて報告される
3. 残高不足で `withdraw` を呼ぶと「責任: 呼び出し側」と表示される

### ステップ1：最初の縦切り — `type ... where` だけを変換する

トランスパイラ全体の骨組み（字句解析 → 構文解析 → 出力）を、一番小さな構文で一本通す。

```cpp
// examples/port.c.c
type Port = u16 where 1 <= self && self <= 65535;
```

- `ccc build port.c.c -o port.cpp` で、ステップ0の手書き版と同等の C++ を出す
- 対応する型：`i32` `i64` `u16` `u64` `f64` `bool` `string`
- 構文エラーは行番号・列番号付きで報告する
- `1 <= self <= 65535` のような連鎖比較もここで扱う

**完了条件**：スナップショットテストが通る。生成した `port.cpp` をテスト用 `main` と一緒にビルドして実行できる

### ステップ2：関数と関数契約

```cpp
contract fn withdraw(Account& a, i64 amt) -> i64
  pre(amt > 0, "金額は正")
  pre(a.balance >= amt, "残高が足りる")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{
  a.balance -= amt;
  return amt;
}
```

- `fn` / `contract fn`、文（`let` `return` `if` `for` 式文）、式（算術・比較・論理・呼び出し・フィールドアクセス）
- `old(x)` は関数冒頭のコピーに変換
- `cpp { ... }` ブロック（逃げ道）もここで入れる。`Account` 構造体などはこれで書ける

**完了条件**：`.c.c` だけで書いたプログラムを実行し、`pre` 違反と `post` 違反でそれぞれ正しい責任者が表示される

### ステップ3：`schema`

- `schema` → `struct`、`parse()`、`to_json()`
- フィールドの `where`、既定値、`T?`、`vec<T>`、他の `type` / `schema` の再利用、`invariant`
- `transform`（`trim` のみ）
- `Raw<Json>` と検証済み型の区別：`Raw<Json>` を `User` として渡すと C++ のコンパイルエラーになる

**完了条件**：コンセプト 4.1 の `User`（`enum` を除く）が変換でき、ステップ0の手書き版と同じエラー全件報告が出る

### ステップ4：効果の検査（最小）

- まずは `io` と `throw` の2つだけ。`effects()` は純粋
- 呼び出し先の効果が呼び出し元に含まれていなければ **C.C. のコンパイルエラー**
- `throw` がない関数には `noexcept` を付ける
- ここで「効果を省略した関数の扱い」（未決事項1）を決める

**完了条件**：コンセプト 6 章の `bad()` がエラーになり、`config()` は通る

### ここで一区切り

ステップ4まで終わったら、実際に小さなプログラム（例：設定ファイルを読んで検証する CLI）を C.C. で書いてみて、構文と未決事項を見直す。その後に進む候補：

- `with` によるハンドラ差し替えと `test` ブロック（フェーズ2の残り）
- 効果の追加（`alloc` `log` `nondet` ...）
- JSON Schema の出力
- Z3 による実行時チェックの削減（フェーズ3）

---

## 4. やらないこと（試作の範囲外）

- 所有（`once` `fixed`）と手順（`protocol`）
- LLVM バックエンド（IR は分けておくが、最初は AST から直接 C++ を出してもよい）
- ジェネリクス、モジュール／import、オーバーロード
- 型推論（`let` の右辺から型を決める程度は除く）
- エディタ支援、パッケージ管理

---

## 5. テストの方針

| 種類 | 内容 |
|---|---|
| スナップショット | `.c.c` → `.cpp` の出力を保存して比較。出力の変化をレビューしやすくする |
| 実行テスト | 生成 C++ を `g++ -std=c++23` でビルドし、標準出力・終了コードを期待値と比較 |
| エラーテスト | 不正な `.c.c` に対するエラーメッセージ（位置と文面）を比較 |

`cargo test` 一つで3種類すべてが走るようにする。

---

## 6. 進捗

| ステップ | 状態 | 確認方法 |
|---|---|---|
| 0. ヘッダーライブラリ | 完了 | `examples/hand/` を手書き。`cargo test` の `hand` / `hand_literal_is_checked_at_compile_time` |
| 1. `type ... where` | 完了 | `examples/port.c.c` |
| 2. 関数と関数契約 | 完了 | `examples/withdraw.c.c`、`tests/run/enforce.c.c` |
| 3. `schema` | 完了（`enum` は未対応） | `examples/user.c.c` |
| 4. 効果の検査 | 完了 | `examples/effects.c.c`、`tests/errors/effects.c.c` |

### 実装中に決めたこと

- **効果を省略した関数（未決事項1）**：検査せず「何でも可」とする。ただし効果を宣言した関数からそれを呼ぶには `unsafe` が必要。既存コードに段階的に注釈を足していける
- **既存 C++ の効果（未決事項2）**：`extern fn` で宣言していない C++ 関数と、関数内の `cpp { }` は `unsafe` 扱い。`extern fn f(...) effects(...);` で効果を絞れる
- **`?`**：失敗時は `cc::Failure` 例外を送出する。したがって `throw` 効果が必要。`main` で捕まえてエラーメッセージを表示し、終了コード 1 で終わる
- **契約違反の位置**：生成コードに `#line` を入れ、`std::source_location` で `.c.c` の行番号を報告する。呼び出し位置は契約付き関数の隠し引数（既定引数）で受け取る
- **スキーマのフィールド**：`where` 付きのフィールドは入れ子の条件付き型（`User::id_t` など）になる。元の型へは暗黙に変換できるので読むときは普通の値として使える
- **対応コンパイラ**：g++ 13 以降。clang 18 は libstdc++ 13 の `std::expected` を使えないため対象外

### 既知の制限（区切りでの見直し候補）

- メソッド呼び出し（`x.size()` など）の効果は追跡していない
- `matches` / `transform` に使えるのは組み込み関数か `extern fn` だけ
- 型の推論は最小限。型が分からない式の誤りは C++ コンパイラのエラーになる
- スキーマに `enum` がない。値を作る手段は `parse` か `cpp` / `extern fn` だけ
