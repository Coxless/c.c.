# C.C. 言語コンセプト（Contract-Centric C++）

- バージョン: ドラフト 0.3（試作用）
- 日付: 2026-10-31
- 拡張子: `.c.c`

---

## 1. 一言でいうと

**一度書いた契約が、型・検証・テスト・ドキュメントのすべてになる C++ ベースの言語。**

値・関数・効果・所有・手順の5つを「契約」として宣言する。コンパイラは証明できる部分を証明し、証明できない部分だけを実行時にチェックする。データの境界では zod のようなスキーマで外部入力を検証し、検証済みの値だけを型として扱う。

### 目指す立ち位置

| 言語 | 主役 | C.C. との違い |
|---|---|---|
| Dafny / F\* | 証明 | 証明できないとビルドが通らない。学習コストが高い |
| C++26 Contracts | 実行時チェック | 関数の pre/post だけ。スキーマも効果もない |
| Cpp2 (cppfront) | 構文の整理 | 契約は持つが効果・スキーマはない |
| Effect-TS / zod | 実用的な検証 | TypeScript 上のライブラリ |
| **C.C.** | **実用→証明への段階的移行** | **まず実行時チェックで使え、後から証明を効かせる** |

---

## 2. 設計原則

1. **契約は一箇所に書く**：型、検証コード、テスト、ドキュメントを別々に書かない。契約から生成する
2. **証明できるものは証明する**：静的に示せる条件はコンパイル時に消し、残りだけを実行時チェックにする
3. **検証ではなく変換（parse）する**：未検証のデータと検証済みのデータを別の型にする。検証済みの型は二度チェックしない
4. **違反には責任者がいる**：契約違反のたびに、呼び出し側と実装側のどちらが破ったかを報告する
5. **出力は普通の C++**：生成コードは読める C++（試作では C++23）。既存のライブラリやツールチェーンをそのまま使える
6. **契約は人と AI の境界**：人間が契約を書き、実装は誰が書いてもよい。AI が書いたコードも契約で機械的に検収する
7. **段階的に導入できる**：全部に契約を書かなくてもよい。外部入力の境界と重要なロジックから始められる

---

## 3. 契約の5層

| 層 | 名前 | 内容 | 参考 |
|---|---|---|---|
| 1 | 値 | 型に条件を付ける（Refinement Types）。スキーマもここ | Liquid Haskell, F\*, zod |
| 2 | 関数 | 事前条件・事後条件・不変条件、`old()` | Eiffel, C++26 Contracts, SPARK |
| 3 | 効果 | 関数が外の世界に何をしてよいか | Koka, OCaml 5, Unison |
| 4 | 所有 | 一回限り・共有・不変の区別 | Rust, Pony, Hylo |
| 5 | 手順 | API を呼べる順序（Typestate） | Typestate, Session Types |

下の層ほど基礎的。試作は層1〜3を対象にする。

---

## 4. 層1：値の契約とスキーマ

### 4.1 構文

```cpp
schema User {
  id:    u64                    where self > 0;
  name:  string                 where 1 <= len(self) <= 50
                                transform trim;
  email: Email;                 // 別スキーマ・型の再利用
  age:   int?                   where 0 <= self <= 150;
  role:  enum { admin, member } = member;      // 既定値
  tags:  vec<string>            where len(self) <= 10 = [];

  invariant(role != admin || age >= 18, "管理者は18歳以上");
}

// 条件付きの型は独立した型（ブランド型）になる
type Email = string matches email_format;
type Port  = u16    where 1 <= self <= 65535;

// 名前の付いた enum。関数の引数にも使える
enum Role { admin, member }
```

- `where` に書けるのはリテラル・`self`・`len()`・算術・比較・論理だけ。後で SMT ソルバーに渡せる範囲に絞っている
- それ以外の条件は `matches`（述語関数）か `invariant`（純粋な関数も呼べる。証明の対象外）に書く
- スキーマにない JSON フィールドはエラーになる。無視したいときは `open schema` と宣言する
- `enum` の値は、型が分かる場所（比較・既定値・引数）では `admin`、それ以外では `Role::admin` と書く

### 4.2 1つの `schema` から生成されるもの

| 生成物 | 内容 |
|---|---|
| `struct User` | 普通の C++ 構造体。フィールドは条件を満たす型 |
| `User::parse()` | JSON などから変換。戻り値は `std::expected<User, ParseError>` |
| `ParseError` | どのフィールドのどの条件が破れたかを**全件**まとめて返す |
| `to_json()` | 直列化。既定値・transform の逆方向も扱う |
| JSON Schema | API ドキュメントや他言語クライアントとの共有用 |
| テスト用生成器 | 条件を満たす値と、わざと破る値を自動生成 |

### 4.3 未検証と検証済みを型で分ける

外部データは `Raw<T>` で受け取り、`parse` を通したものだけが `T` になる。

```cpp
fn handle(body: Raw<Json>) -> Response effects(io, throw) {
  let user = User::parse(body)?;   // ここが信頼境界
  register(user);                  // 以降は検証済み
}

fn register(u: User) effects(io) { ... }
register(body);  // ✗ Raw<Json> は User ではない
```

### 4.4 静的に分かる場合は実行時チェックを消す

- `let p: Port = 8080;` → コンパイル時に確定。チェックコードは生成しない
- `let p: Port = n + 1;` → 静的に分からないのでエラー。`Port::try_from(n + 1)?` を要求する（暗黙の実行時チェックは入れない）

### 4.5 失敗を扱う

`T::parse` と `T::try_from` の結果は、成功と失敗のどちらかである。取り出し方は3つある。

```cpp
let user = User::parse(body)?;      // 失敗を送出する。throw が必要

match User::parse(body) {           // その場で分ける。ok / err の両方が必要
  ok(user) { register(user); }
  err(e)   { eprint(e); }
}

match user.age {                    // T? は some / none で分ける
  some(age) { print(age); }
  none      { print("年齢なし"); }
}

try {                               // 送出された失敗を捕まえる。中の throw は外に出ない
  let text = read_file(path);
} catch message {
  eprint(message);
}
```

---

## 5. 層2：関数の契約と責任の追跡

```cpp
contract fn withdraw(a: Account&, amt: Money) -> Money
  effects(log)
  pre(amt > 0,          "金額は正")
  pre(a.balance >= amt, "残高が足りる")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{
  a.balance -= amt;
  log("withdraw");
  return amt;
}
```

- `pre` の違反 → **呼び出し側**の責任
- `post` の違反 → **実装側**の責任

違反レポートの例:

```
contract violation: pre「残高が足りる」
  責任: 呼び出し側  checkout.c.c:42  pay() → withdraw()
  値:   a.balance = 300, amt = 500
```

---

## 6. 層3：効果の契約

関数が使ってよい効果を宣言する。**呼び出す関数の効果は、呼び出し元が宣言した効果に含まれていなければならない。**

| 効果 | 意味 | 主な用途 |
|---|---|---|
| `io` | ファイル、ネットワーク、標準入出力 | パーサが外部通信しない保証 |
| `alloc` | ヒープ確保 | `!alloc` でリアルタイム処理 |
| `throw` | 例外の送出 | なければ `noexcept` に変換 |
| `async` | コルーチンによる中断 | 同期処理の中での誤用防止 |
| `unsafe` | 生ポインタ、未定義動作の可能性 | 監査範囲の絞り込み |
| `nondet` | 時刻、乱数 | テストの再現性 |
| `log` | ログ出力 | `io` から分離した軽い効果 |

```cpp
fn parse(s: str) -> Json effects()          // 純粋
fn load(p: Path) -> str  effects(io, throw)

fn config() -> Json effects(io, throw) {
  return parse(load("a.json"));  // OK
}
fn bad(s: str) -> Json effects() {
  load("x");  // ✗ io も throw も宣言していない
}
```

効果を省略した関数は検査されず、何でもできる（既存コードから段階的に導入するため）。効果が分からないもの、つまり効果を省略した関数、宣言のない C++ 関数、関数内の `cpp { }`、C++ のメソッド呼び出しを呼ぶには `unsafe` が必要になる。既存の C++ 関数は `extern fn f(...) effects(...);` で効果を宣言して使う。

### 6.1 エフェクトハンドラ（効果の差し替え）

```cpp
fn load_config() -> Config effects(io, throw) {
  return Config::parse(parse_json(read_file("app.json")))?;
}

test "設定を読める" {
  with io = MemoryFs{ {"app.json", R"({"port": 8080})"} } {
    expect(load_config().port == 8080);
  }
}
```

モックライブラリなしで、同じコードを本番とテストで動かせる。`with` は `test` の中でだけ使える。試作で差し替えられるのは `io` の `read_file` だけ。

テストは `ccc test` で実行する。テストごとに別プロセスで動くので、契約違反で止まっても残りのテストは実行される。

---

## 7. 層4・5：所有と手順（試作の対象外。方向性のみ）

```cpp
once Token t = issue();     // 一回限り。使うと消える
spend(t);
spend(t);                   // ✗ use-after-move

fixed Config cfg = load();  // 深い不変

protocol File {
  Closed -> open()  -> Opened;
  Opened -> read()  -> Opened;
  Opened -> close() -> Closed;
}
File f;  f.read();          // ✗ Closed 状態では read() を呼べない
```

---

## 8. 処理パイプライン

前の段階で片付いた契約は、後の段階に残らない。

1. **型検査**：効果・所有・手順・スキーマの構造をチェック
2. **静的証明**：条件式を SMT ソルバー（Z3）に渡し、証明できたものを消す
3. **実行時チェックの挿入**：残った条件を自前の実行時チェックとして出力。既定は enforce で、observe に切り替えられる
4. **テスト生成**：スキーマと pre からプロパティベーステストを作り、post を検査
5. **ドキュメント生成**：API リファレンスと JSON Schema を出力

---

## 9. 実装方式：トランスパイラ（将来はコンパイラにも）

### 9.1 構成

```
.c.c ─▶ 字句解析 ─▶ 構文解析 ─▶ AST ─▶ 意味解析 ─▶ C.C. IR ─┬▶ C++ 出力（最初）
                                         (型・効果・契約)        └▶ LLVM IR 出力（将来）
```

### 9.2 C++ を出力する理由

- STL や既存ライブラリとの相互運用がほぼタダで手に入る（テンプレート展開・オーバーロード解決を自作しなくてよい）
- 最適化、各 CPU 向けのコード生成、デバッガ、sanitizer をそのまま使える
- 生成物が読めるので、言語が廃れても成果物が残る
- 前例：Cfront（C++ 自身）、cppfront、TypeScript、Nim

C.C. の価値（スキーマ・効果・証明・責任追跡）はすべて**フロントエンド側**にあるので、どちらの方式でも作業量の中心は同じ。**IR をきちんと分離しておけば、後から LLVM バックエンドを追加できる。**

### 9.3 C++ への変換表

| C.C. | 生成される C++ |
|---|---|
| `schema User {...}` | struct、parse 関数、`std::expected`、シリアライザ |
| `type Port = u16 where ...` | 検査付きコンストラクタを持つ薄いラッパークラス |
| `enum Role {...}` | `enum class` と、値の名前の一覧 |
| `pre` / `post` | 自前の実行時チェック（`cc::violation`）。責任の報告に呼び出し位置と `.c.c` の条件文が要るため |
| `match` / `try` | `std::expected` の分岐 / `try` と `catch` |
| `old(x)` | 関数冒頭での値のコピーと `post` での比較 |
| `effects(...)` | コンパイル時に検査して消去。`throw` がなければ `noexcept` |
| `with io = ...` | スレッドローカルのハンドラをスコープの間だけ差し替える |
| `test "..." {...}` | `CC_TEST` を定義したときだけ生成される関数と、テストを実行する `main` |
| `once` / `fixed` | ムーブ専用型 / 深い const ラッパー |
| `protocol` | 状態ごとの型を返すメソッド（typestate パターン） |

### 9.4 変換例

入力:

```cpp
type Port = u16 where 1 <= self <= 65535;
```

出力（イメージ）:

```cpp
class Port {
  std::uint16_t v_;
  explicit constexpr Port(std::uint16_t v) : v_(v) {}
public:
  static constexpr std::expected<Port, RefineError> try_from(std::uint16_t v) {
    if (!(1 <= v && v <= 65535)) return std::unexpected(RefineError{"Port", "1 <= self <= 65535"});
    return Port{v};
  }
  template <std::uint16_t V> requires (1 <= V && V <= 65535)
  static consteval Port of() { return Port{V}; }   // リテラル用：チェックはコンパイル時
  constexpr std::uint16_t get() const { return v_; }
};
```

---

## 10. 試作ロードマップ

| フェーズ | 内容 | 完了条件 | 状況 |
|---|---|---|---|
| **0. ヘッダーライブラリ** | トランスパイラなしで `expected` を返す parse、契約マクロを C++ で書く | 手書き C++ で User の parse とエラー全件報告が動く | 完了 |
| **1. スキーマ＋関数契約** | `schema`、`type ... where`、`pre/post/old` を変換するトランスパイラ。実行時チェックのみ | `.c.c` → `.cpp` → g++ でビルドし、違反レポートが出る | 完了 |
| **2. 効果システム** | `effects(...)` の検査、`with` によるハンドラ差し替え、`test` ブロック | 効果違反がコンパイルエラーになる。MemoryFs でテストが通る | 完了 |
| **3. 静的証明とテスト生成** | Z3 連携で実行時チェックを削減。スキーマからテスト自動生成 | リテラルや単純な算術の条件がチェックなしになる | 未着手 |
| **4. 所有と手順** | `once`、`fixed`、`protocol` | use-after-move と手順違反を検出 | 未着手 |

JSON Schema の出力（4.2）も未着手。

### 10.1 フェーズ1の最小スコープ（MVP）

- 型：`i32` `i64` `u16` `u64` `f64` `bool` `string` `vec<T>` `T?`
- 宣言：`fn`、`contract fn`、`schema`、`type ... where`
- 式：算術、比較、論理、`len()`、フィールドアクセス、関数呼び出し
- 文：`let`、`return`、`if`、`for`、式文
- 契約：`pre`、`post`、`old()`、`invariant`
- 入出力：JSON のみ（実装は nlohmann/json などの既存ライブラリを利用）
- C++ 直書き：`cpp { ... }` ブロックで生の C++ を埋め込める（逃げ道）

### 10.2 技術選択の候補

| 項目 | 候補 |
|---|---|
| 実装言語 | Rust（決定） |
| パーサ | 手書き再帰下降（決定。エラーメッセージを作り込みやすい） |
| 出力先 | C++23（決定。g++ 13 以降。clang 18 + libstdc++ 13 は `std::expected` を使えない） |
| SMT | Z3（フェーズ3〜） |

---

## 11. 決めたこと（元の未決事項）

試作の CLI（`examples/confcheck.c.c`）を書いて見直した結果。経緯は [PLAN.md](PLAN.md) の7章にある。

1. **効果を省略した関数**：「何でも可」とする。ただし、効果を宣言した関数から呼ぶには `unsafe` が必要
2. **既存 C++ の効果**：既定で `unsafe` とし、`extern fn` で絞る。C++ のメソッド呼び出しも同じ扱い。生成された `to_json` と `get` だけは純粋とする
3. **スキーマのエラー型**：フィールドのパス（`users[3].email`）を含む構造化データで、全件返す
4. **`where` に書ける式**：リテラル・`self`・`len()`・算術・比較・論理に限る。任意の純粋関数は `invariant` に書く
5. **構文の方向性**：名前と型の順を `名前: 型` に揃える（引数・フィールド・`let`）。文や式は C++ に寄せる
6. **C++26 Contracts への依存度**：自前の実行時チェックを主にする

---

## 12. 実現するとうれしいこと（動機の確認用）

- 入力チェックの書き忘れが型エラーになる（`Raw<T>` と `T` の区別）
- 構造体・バリデーション・JSON 変換・ドキュメント・テストデータの二重管理がなくなる
- 契約違反レポートで、バグの責任がどちら側にあるかすぐ分かる
- シグネチャを見るだけで「外部通信しない」「メモリ確保しない」と分かる
- 依存ライブラリが不審な効果を要求していたら気づける
- モックなしでテストが書ける
- AI が書いた実装を契約で機械的に検収できる（人間は契約をレビューする）
- 既存の C++ 資産と性能を捨てずに導入できる

---

## 付録：参考

- C++26 Contracts: WG21 で 2026年3月に C++26 とともに承認。仮想関数への対応は先送り
  - https://www.devclass.com/development/2026/04/01/contracts-are-in-c26-despite-disquiet-over-their-value/5213555
  - https://isocpp.org/files/papers/P4043R0.html
- Effectful Software Contracts (POPL 2024)
  - https://popl24.sigplan.org/details/POPL-2024-popl-research-papers/90/Effectful-Software-Contracts
- 関連言語・ライブラリ: Dafny, F\*, Ada/SPARK, Koka, Eiffel, Racket contracts, cppfront, zod, Effect-TS, Verus
