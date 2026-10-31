# C.C. 言語コンセプト（Contract-Centric C++）

- バージョン: ドラフト 0.5
- 日付: 2026-10-31
- 拡張子: `.c.c`（ヘッダーは `.c.h`）

> [!NOTE]
> この文書は考え方と目指す形を書いたものです。今の `ccc` が受け付ける言語の定義は [docs/language.md](docs/language.md)、作業の状況は [PLAN.md](PLAN.md) にあります。

---

## 1. 一言でいうと

**C++ に対する TypeScript のような言語。普通の C++ に契約を書き足すと、型・検証・テスト・ドキュメントになる。**

JavaScript がほぼそのまま TypeScript として通るように、C++ のコードはそのまま C.C. として通ります。そこに値・関数・効果の契約を少しずつ足していくと、その部分から検査が効き始めます。

### TypeScript との対応

| TypeScript | C.C. |
|---|---|
| JS はほぼそのまま TS として通る | `.cpp` を `.c.c` に改名すれば通る |
| 型注釈を足すほど検査が効く | `schema`、`pre` / `post`、`effects` を足すほど検査が効く |
| `any` | 注釈のない関数、`unsafe` |
| `strict` | 厳格モード（`#pragma cc strict`） |
| `declare`、`.d.ts` | 既存の関数への効果の宣言 |
| 健全性より実用性を優先する | 検査できない部分は `unsafe` で明示する |

**TypeScript と違う点:** TypeScript の型は変換すると消えますが、C.C. の拡張は `parse()` や実行時チェックのコードを生成します。「TypeScript と zod を合わせたもの」に近いです。

### 目指す立ち位置

| 言語 | 主役 | C.C. との違い |
|---|---|---|
| Dafny / F\* | 証明 | 証明できないとビルドが通らない。学習コストが高い |
| C++26 Contracts | 実行時チェック | 関数の pre/post だけ。スキーマも効果もない |
| Cpp2 (cppfront) | 構文の整理 | 契約は持つが効果・スキーマはない |
| Carbon | C++ の後継 | 新しい文法への移行が前提。C.C. は C++ のまま書く |
| Effect-TS / zod | 実用的な検証 | TypeScript 上のライブラリ |
| **C.C.** | **C++ への段階的な契約の導入** | **既存の C++ に足していける。証明は後から効かせる** |

---

## 2. 設計原則

1. **C++ はそのまま通す**：有効な C++ は有効な C.C. である。拡張はファイルの一部にだけ現れる
2. **C++ の意味を変えない**：拡張の外にある C++ は、C++ のとおりに動く。挙動を厳しくするのは、書き手が厳格モードを選んだときだけ
3. **足す構文は最小にする**：ライブラリか C++ の機能で書けるものは、構文にしない
4. **契約は一箇所に書く**：型、検証コード、テスト、ドキュメントを別々に書かない。契約から生成する
5. **検証ではなく変換（parse）する**：未検証のデータと検証済みのデータを別の型にする。検証済みの型は二度チェックしない
6. **違反には責任者がいる**：契約違反のたびに、呼び出し側と実装側のどちらが破ったかを報告する
7. **検査はコンパイラに任せる**：C++ の解析を自作しない。呼び出しの解決などは Clang の結果を使う
8. **出力は普通の C++**：生成コードは読める C++。既存のライブラリやツールチェーンをそのまま使える
9. **検査できないものは正直に示す**：完全な保証はあきらめ、信頼している箇所を `unsafe` として見えるようにする
10. **契約は人と AI の境界**：人間が契約を書き、実装は誰が書いてもよい。AI が書いたコードも契約で機械的に検収する

---

## 3. 契約の5層

| 層 | 名前 | 内容 | 検査するもの |
|---|---|---|---|
| 1 | 値 | 型に条件を付ける（Refinement Types）。スキーマもここ | C++ の型システム、`ccc` のコード生成 |
| 2 | 関数 | 事前条件・事後条件、`old()` | `ccc` のコード生成 |
| 3 | 効果 | 関数が外の世界に何をしてよいか | Clang の解析 |
| 4 | 所有 | 一回限り・共有・不変の区別 | ムーブ専用型、Clang の解析（方向性のみ） |
| 5 | 手順 | API を呼べる順序（Typestate） | 状態ごとの型、Clang の解析（方向性のみ） |

---

## 4. 足す構文

C++ に足すのは次の5つだけです。**それ以外はすべて普通の C++ です。**

| 構文 | 役割 | 検出の方法 |
|---|---|---|
| `type T = ... where ...;` | 条件付きの型 | トップレベルの文脈で判定 |
| `schema T { ... }` | スキーマ | 同上 |
| `enum T { ... }` のうちスキーマで使うもの | 名前の一覧付きの列挙（宣言は普通の C++ のまま） | スキーマから参照されたとき |
| `pre(...)` / `post(...)` | 関数の契約 | 関数の宣言の `)` の直後 |
| `test "名前" { ... }` | テスト | トップレベルの文脈で判定 |

`[[cc::effects(...)]]` は C++ の属性の構文なので、新しい文法ではありません。

`type` や `schema` は、C++ では変数名や型名にも使えます。形で判定するので、`type x = 1;`（`where` がない）や `schema s{1};`（フィールドがない）は C++ のままです。同じ名前をマクロにしているファイルでは、その語は拡張として扱いません。TypeScript の文脈依存キーワードと同じ考え方です。

### C++ の機能で済ませるもの

| やりたいこと | 書き方 |
|---|---|
| 型の短い名前 | `<cc/cc.hpp>` の `cc::i64`、`cc::vec<T>` など。`type` と `schema` の中では `cc::` なしで書ける |
| 失敗を例外にする | `cc::must(User::parse(j))` |
| 失敗をその場で分ける | `if (auto r = User::parse(j)) { ... } else { ... }` |
| 既存の関数に効果を宣言する | 普通の宣言に `[[cc::effects(...)]]` を付ける |
| テストで効果を差し替える | `cc::MemoryFs fs{...}; cc::WithIo io{fs};` |

---

## 5. 層1：値の契約とスキーマ

### 5.1 構文

```cpp
type Port  = u16    where 1 <= self <= 65535;
type Email = string matches email_format;

enum Role { admin, member };

schema User {
  id:    u64                    where self > 0;
  name:  string                 where 1 <= len(self) <= 50
                                transform trim;
  email: Email;
  age:   int32?                 where 0 <= self <= 150;
  role:  Role = member;
  tags:  vec<string>            where len(self) <= 10 = [];

  invariant(role != admin || age >= 18, "管理者は18歳以上");
}
```

- `where` に書けるのはリテラル・`self`・`len()`・算術・比較・論理だけ。後で SMT ソルバーに渡せる範囲に絞っている。**連鎖比較（`1 <= x <= 9`）は `where` と `invariant` の中でだけ使える**
- それ以外の条件は `matches`（述語関数）か `invariant`（純粋な関数も呼べる。証明の対象外）に書く
- スキーマにない JSON フィールドはエラーになる。無視したいときは `open schema` と宣言する

### 5.2 1つの `schema` から生成されるもの

| 生成物 | 内容 | 状況 |
|---|---|---|
| `struct User` | 普通の C++ 構造体。フィールドは条件を満たす型 | 実装済み |
| `User::parse()` | JSON から変換。戻り値は `std::expected<User, ParseError>` | 実装済み |
| `ParseError` | どのフィールドのどの条件が破れたかを**全件**まとめて返す | 実装済み |
| `User::make()` | プログラムの中の値から作る。`parse` と同じ検査を通す | 実装済み |
| `to_json()` | 直列化 | 実装済み |
| JSON Schema | API ドキュメントや他言語クライアントとの共有用 | 未着手 |
| テスト用生成器 | 条件を満たす値と、わざと破る値を自動生成 | 未着手 |

`invariant` を持つスキーマのフィールドは `const` になり、外から作る手段は `parse` と `make` だけになります。`u.age = 3;` はコンパイルエラーです。値全体の代入（`a = b;`）はできます。どちらも検査済みの値なので、不変条件は破れません。`invariant` のないスキーマは普通の構造体で、条件付きのフィールドが自分で値を守ります。

### 5.3 未検証と検証済みを型で分ける

外部データは `cc::Raw<T>` で受け取り、`parse` を通したものだけが `T` になります。C++ の型の検査だけで成り立つので、Clang は要りません。

```cpp
void handle(cc::Raw<cc::Json> body) {
  User user = cc::must(User::parse(body));   // ここが信頼境界
  register_user(user);                       // 以降は検証済み
}

void register_user(const User& u);
register_user(body);  // ✗ コンパイルエラー: Raw<Json> は User ではない
```

### 5.4 静的に分かる場合は実行時チェックを消す

```cpp
Port a = 8080;                  // コンパイル時に検査。チェックコードは生成しない
Port b = 0;                     // ✗ コンパイルエラー
Port c = n + 1;                 // ✗ コンパイルエラー。静的に分からない
auto d = Port::try_from(n + 1); // 実行時に検査。std::expected を返す
```

`consteval` の変換コンストラクタで実現します。`try_from` は、受け取った整数が元の型の範囲に収まるかを、変換する前に調べます。

### 5.5 失敗を扱う

新しい構文は足しません。

```cpp
User user = cc::must(User::parse(body));    // 失敗すると例外を送出する

if (auto r = User::parse(body)) {           // その場で分ける
  register_user(*r);
} else {
  std::cerr << r.error().to_string();
}
```

---

## 6. 層2：関数の契約と責任の追跡

```cpp
std::int64_t withdraw(Account& a, std::int64_t amt)
  pre(amt > 0,          "金額は正")
  pre(a.balance >= amt, "残高が足りる")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{
  a.balance -= amt;
  return amt;
}
```

- `pre` の違反 → **呼び出し側**の責任
- `post` の違反 → **実装側**の責任
- `pre` / `post` の位置と `post(r: ...)` の形は、C++26 Contracts に揃える。説明の文字列と `old()` は C.C. の拡張

違反レポートの例:

```
contract violation: pre「残高が足りる」
  条件: a.balance >= amt
  責任: 呼び出し側  checkout.c.c:42  pay() → withdraw()
  契約: checkout.c.c:11
  値:   a.balance = 300, amt = 500
```

本体を解析せずに `post` を検査するため、関数を「元の本体」と「検査して呼び出す外側の関数」に分けて生成します。呼び出し位置は外側の関数の隠し引数で受け取ります。`post` から見える値渡しの引数は、呼び出されたときの値です。

`pre` / `post` は関数の定義に書きます。隠し引数でシグネチャが変わるので、関数ポインタに入れるときは型が変わることに注意してください。

---

## 7. 層3：効果の契約

関数が使ってよい効果を宣言します。**呼び出す関数の効果は、呼び出し元が宣言した効果に含まれていなければなりません。**

```cpp
[[cc::effects()]]                 // 純粋
int add(int a, int b) { return a + b; }

[[cc::effects(io, throw)]]
std::string load(const std::string& path);

[[cc::effects()]]
int bad() {
  load("x");                      // ✗ io と throw を宣言していない
  return 0;
}
```

| 効果 | 意味 | 主な用途 |
|---|---|---|
| `io` | ファイル、ネットワーク、標準入出力 | パーサが外部通信しない保証 |
| `alloc` | ヒープ確保。`io` は `alloc` を含む（入出力は確保なしでは書けないため）。メモリ不足の `std::bad_alloc` は `throw` に数えない | リアルタイム処理 |
| `throw` | 例外の送出 | 例外が漏れないことの保証 |
| `async` | コルーチンによる中断 | 同期処理の中での誤用防止 |
| `unsafe` | 生ポインタ、未定義動作の可能性、呼び先の分からない呼び出し | 監査範囲の絞り込み |
| `nondet` | 時刻、乱数 | テストの再現性 |
| `log` | ログ出力 | `io` から分離した軽い効果 |

### 7.1 Clang で検査する理由

C++ では、呼び出しがソースに見えないことがあります。暗黙のコンストラクタ、デストラクタ、演算子、テンプレートの実体、マクロ、引数の型による名前の探索（ADL）です。正しく判定するにはコンパイラ並みの解析が要るので、自作せず Clang の AST を読みます。

| 機能 | Clang |
|---|---|
| `type`、`schema`、`Raw<T>`、`pre` / `post`、厳格モード、ビルド | 不要（g++ で可） |
| **`effects` の検査** | **必要** |

位置づけは ESLint に近いものです。`ccc check` のときだけ Clang で効果を検査します。`[[cc::effects]]` のないファイルでは Clang を起動しません。Clang がない環境では、効果の検査を飛ばして警告を出します。必要なのは clang 19 以降です（libstdc++ 13 の `std::expected` を読むため）。

### 7.2 検査の規則

- `[[cc::effects(...)]]` を付けた関数の本体だけを検査する。**注釈のない関数は普通の C++ のままで、何でもできる**
- 呼び先は暗黙の呼び出しも含めて、Clang が解決した結果で判定する。テンプレートは実体ごとに判定する
- ラムダの本体は、それを書いた関数の一部として検査する

| 呼び先 | 効果 |
|---|---|
| `[[cc::effects(...)]]` のある関数 | 宣言されたもの |
| 効果の一覧（標準ライブラリ。[compiler/src/effect_table.rs](compiler/src/effect_table.rs)）にある関数 | 一覧のもの |
| 注釈のない関数、一覧にない関数 | `unsafe` |
| 仮想関数、関数ポインタ、`std::function` | `unsafe` |
| `throw` 式 | `throw`。`catch (...)` のある `try` の中では外に出ないので不要 |
| `new`、`delete` | `alloc` |
| コルーチンの中断（`co_await` など） | `async` |
| `reinterpret_cast`、インラインアセンブリ | `unsafe` |

`pre` / `post` の条件と `old()` のコピーも関数の一部として検査します。

効果の推論はしません。シグネチャを読めば効果が分かることに価値があるからです。

### 7.3 エフェクトハンドラ（効果の差し替え）

```cpp
[[cc::effects(io, throw)]]
Config load_config() {
  return cc::must(Config::parse(cc::parse_json(cc::read_file("app.json"))));
}

test "設定を読める" {
  cc::MemoryFs fs{ {"app.json", R"({"port": 8080})"} };
  cc::WithIo io{fs};
  expect(load_config().port == 8080);
}
```

モックライブラリなしで、同じコードを本番とテストで動かせます。差し替えられるのは `cc::read_file` のような C.C. のライブラリ経由の呼び出しだけで、`fopen` の直接呼び出しは差し替わりません（効果の検査では `unsafe` として見つかります）。

テストは `ccc test` で実行します。テストごとに別プロセスで動くので、契約違反で止まっても残りのテストは実行されます。

---

## 8. 厳格モード

原則2のとおり、既定では C++ の挙動を変えません。ファイルの先頭に `#pragma cc strict` と書くと、そのファイルで次が有効になります。

| 検査 | 実現方法 |
|---|---|
| 値が変わる暗黙の変換、符号付きと符号なしの比較 | g++ の警告をエラーにする |
| `return` のない経路 | 同上 |
| `v[i]` の範囲外 | libstdc++ の検査を有効にする |

`ccc run` と `ccc test` は厳格モードに関係なく UBSan を有効にしてビルドし、オーバーフローやゼロ除算を実行時に検出します。

---

## 9. 層4・5：所有と手順（方向性のみ）

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
```

ムーブ専用型と状態ごとの型に変換し、検出には Clang の解析を使う想定です。具体的な設計は未定です。

---

## 10. 処理パイプライン

```
.c.c ─▶ ccc（拡張だけ変換）─▶ .cpp ─┬▶ Clang の AST ─▶ 効果の検査（ccc check）
                                    └▶ g++ または clang++ ─▶ 実行ファイル
```

1. **変換**：`ccc` が字句を読み、拡張だけを C++ に置き換える。**それ以外は1文字も変えない**。`#line` で `.c.c` の行を保つ
2. **ビルド**：普通の C++ コンパイラでビルドする。値の契約の多くはここで検査される
3. **効果の検査**：Clang の AST を読む
4. **静的証明（将来）**：`where` と `pre` を SMT ソルバー（Z3）に渡し、証明できたものの実行時チェックを消す
5. **生成（将来）**：プロパティベーステスト、JSON Schema、API リファレンス

---

## 11. 変換例

```cpp
type Port = u16 where 1 <= self <= 65535;
```

出力（要点）:

```cpp
class Port {
public:
  static constexpr bool check(const std::uint16_t& self) { return 1 <= self && self <= 65535; }
  template <cc::plain_integer A>
  consteval Port(A v) : v_(static_cast<std::uint16_t>(v)) {  // リテラル用：検査はコンパイル時
    if (!cc::fits<std::uint16_t>(v) || !check(static_cast<std::uint16_t>(v))) cc::literal_rejected();
  }
  template <cc::integer_like A>
  static constexpr std::expected<Port, cc::RefineError> try_from(const A& a);  // 実行時：先に範囲を調べる
  constexpr const std::uint16_t& get() const { return v_; }
  constexpr operator const std::uint16_t&() const { return v_; }
private:
  constexpr Port(cc::Unchecked, std::uint16_t v) : v_(v) {}
  std::uint16_t v_;
};
```

`cc::literal_rejected` は `constexpr` ではない関数です。条件を満たさないリテラルはそこに届くのでコンパイルできず、`ccc` がそのエラーを「リテラル 0 は Port の条件を満たしていません」と言い換えます。

---

## 12. ロードマップ

| フェーズ | 内容 | 状況 |
|---|---|---|
| **0. ヘッダーライブラリ** | `expected` を返す parse、契約の実行時チェック | 完了 |
| **1. 値と関数の契約** | `schema`、`type`、`pre` / `post` | 完了（`pre` / `post` は普通の関数まで） |
| **2. 効果** | Clang による効果の検査、ハンドラの差し替え、`test` | 完了 |
| **3. 静的証明とテスト生成** | Z3 連携、テスト自動生成、JSON Schema | 未着手 |
| **4. 所有と手順** | `once`、`fixed`、`protocol` | 未着手 |

技術の選択:

| 項目 | 決定 |
|---|---|
| `ccc` の実装言語 | Rust |
| 拡張の解析 | 手書きの字句解析と再帰下降。C++ の部分は解析しない |
| 出力 | C++23 |
| ビルド | g++ 13 以降（`CXX` で変更可） |
| 効果の検査 | Clang の AST（`-ast-dump=json` をストリームで読む） |
| 契約の実行時チェック | 自前（`cc::violation`）。責任の報告に呼び出し位置と `.c.c` の条件文が要るため |
| SMT | Z3（フェーズ3〜） |

---

## 13. 決めたこと

| 項目 | 決定 |
|---|---|
| 言語の形 | **C++ の上位互換**。書き慣れた C++ で書け、既存コードにも足せるため |
| 効果を省略した関数 | 検査しない。効果を宣言した関数から呼ぶと `unsafe` |
| 既存 C++ の効果 | 既定で `unsafe`。`[[cc::effects]]` か効果の一覧で絞る |
| スキーマのエラー型 | パス付きの構造化データを全件返す |
| `where` に書ける式 | リテラル・`self`・`len()`・算術・比較・論理に限る |
| 構文の方向性 | C++ のまま。拡張の中だけ `名前: 型` |
| C++26 Contracts への依存度 | 実行時チェックは自前。構文は揃える。説明の文字列と `old()` は拡張 |
| 効果の検査方法 | Clang の AST |
| `noexcept` | 自動では付けない。C++ の意味を変えないため |
| `post` から見える引数 | 値渡しの引数は呼び出されたときの値。C++26 でも `post` から見える引数は `const` なので矛盾しない |
| `invariant` と代入 | `invariant` を持つスキーマのフィールドを `const` にする |
| スキーマの値を作る手段 | `User::make({.id = 1, ...})`。`parse` と同じ検査を通す |
| スキーマの `enum` | 普通の `enum` / `enum class` に、スキーマから参照されたときだけ名前の一覧を足す。値の指定（`a = 5`）は不可 |
| `pre` / `post` を書く場所 | 関数の定義。宣言だけの関数に書くとエラー |
| マクロの中の拡張 | 対応しない |

---

## 14. 実現するとうれしいこと（動機の確認用）

- 入力チェックの書き忘れが型エラーになる（`Raw<T>` と `T` の区別）
- 構造体・バリデーション・JSON 変換・ドキュメント・テストデータの二重管理がなくなる
- 契約違反レポートで、バグの責任がどちら側にあるかすぐ分かる
- シグネチャを見るだけで「外部通信しない」「メモリ確保しない」と分かる
- 依存ライブラリが不審な効果を要求していたら気づける
- モックなしでテストが書ける
- AI が書いた実装を契約で機械的に検収できる（人間は契約をレビューする）
- **既存の C++ を書き直さず、重要な箇所から導入できる**

---

## 付録：参考

- C++26 Contracts: WG21 で 2026年3月に C++26 とともに承認。仮想関数への対応は先送り
  - https://www.devclass.com/development/2026/04/01/contracts-are-in-c26-despite-disquiet-over-their-value/5213555
  - https://isocpp.org/files/papers/P4043R0.html
- Effectful Software Contracts (POPL 2024)
  - https://popl24.sigplan.org/details/POPL-2024-popl-research-papers/90/Effectful-Software-Contracts
- 関連言語・ライブラリ: TypeScript, Dafny, F\*, Ada/SPARK, Koka, Eiffel, Racket contracts, cppfront, Carbon, zod, Effect-TS, Verus
- Clang の関連機能: 関数の効果の属性（`[[clang::nonblocking]]` など）、`-Wconsumed`、`-Wthread-safety`、clang-tidy
