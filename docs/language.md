# C.C. 言語仕様（試作版）

今の `ccc` が実際に受け付ける言語を定めた文書です。考え方と将来の構想は [CONCEPT.md](../CONCEPT.md) にあります。

- 対象の環境は Ubuntu 24.04、g++ 13、clang 19 / 20 です
- 文書と実装が食い違っていたら、今の正解は実装です
- 互換性は保証しません。構文も挙動も変わる可能性があります

**先に読むとよい節:** [最初のプログラム](#1-最初のプログラム)、[つまずきやすい点](#11-つまずきやすい点)

## 目次

1. [最初のプログラム](#1-最初のプログラム)
2. [C++ との関係](#2-c-との関係)
3. [条件付きの型：`type`](#3-条件付きの型type)
4. [スキーマ：`schema`](#4-スキーマschema)
5. [関数の契約：`pre` と `post`](#5-関数の契約pre-と-post)
6. [効果：`[[cc::effects]]`](#6-効果cceffects)
7. [厳格モード](#7-厳格モード)
8. [テスト：`test`](#8-テストtest)
9. [拡張付きヘッダー：`.c.h`](#9-拡張付きヘッダーch)
10. [`<cc/cc.hpp>` のライブラリ](#10-cccchpp-のライブラリ)
11. [つまずきやすい点](#11-つまずきやすい点)
12. [`ccc` のコマンドと終了コード](#12-ccc-のコマンドと終了コード)
13. [文法](#13-文法)

---

## 1. 最初のプログラム

普通の C++ は、そのまま `.c.c` として動きます。

```cpp
#include <iostream>

int main() {
  std::cout << "hello\n";
}
```

```sh
ccc run hello.c.c
```

ここに契約を足します。

```cpp
#include <iostream>

type Percent = i64 where 0 <= self <= 100;

int discounted(int price, Percent off)
  pre(price >= 0, "価格は負でない")
  post(r: r <= price)
{
  return price - price * off / 100;
}

int main() {
  std::cout << discounted(200, 25) << "\n";   // 150
  Percent wrong = 120;                        // コンパイルエラー
}
```

## 2. C++ との関係

- 有効な C++ は有効な C.C. です。拡張の外の文字は1文字も変わりません
- 変換後のコードは C++23 です。`ccc run` などは g++ に `-std=c++23` を渡します
- 拡張を使うファイルには、`ccc` が先頭に `#include <cc/cc.hpp>` を足します。拡張を使わないファイルには何も足しません（`#line` だけです）
- コンパイルエラーと契約違反の報告は `.c.c` の行番号を指します

### 拡張の判定

語だけでは拡張と判定しません。次の形のときだけです。**同じ名前のマクロ（`#define pre(...)` など）があるファイルでは、その語は拡張になりません。**

| 語 | 拡張になる形 | C++ のままのもの |
|---|---|---|
| `type` | 名前空間のスコープの宣言の先頭で `type 名前 =` に続き、`;` までに `where` か `matches` がある | `type x = 1;` |
| `schema` | 同じく `[open] schema 名前 {` に続き、`名前 :` か `invariant(` がある | `schema s{1};`、`schema s{};` |
| `enum` | スキーマのフィールドの型として参照されている | ほかのすべての `enum` |
| `pre`、`post` | 関数の宣言子（`)`、`noexcept`、後置の戻り値の型）の直後で、`(...)` の後に `{`、`;`、別の `pre` / `post` が続く。関数の本体の中は対象外 | `int pre(int x);`、`x.pre(1)`、`if (a) pre(b);` |
| `test` | ファイルのトップレベルの宣言の先頭で `test "文字列" {` | `test::value` |

コメント、文字列、プリプロセッサの行の中は対象になりません。マクロの定義の中に拡張を書くことはできません。

### 名前の衝突

`self`、`where` などが意味を持つのは `type` と `schema` の中だけです。`len`、`trim`、`email_format` は、`where`、`invariant`、`matches`、`transform` の中では `cc::` の関数を指します。

## 3. 条件付きの型：`type`

```cpp
type Port  = u16    where 1 <= self <= 65535;
type Email = string matches email_format;
```

名前空間のスコープに書きます。関数の中やクラスの中に書くとエラーです。

### 元の型

| 書き方 | C++ の型 |
|---|---|
| `i32`、`int32`、`int` | `std::int32_t` |
| `i64`、`int64` | `std::int64_t` |
| `u8`、`uint8`、`u16`、`uint16`、`u32`、`uint32`、`u64`、`uint64` | `std::uint8_t` など |
| `f64`、`double` | `double` |
| `bool` | `bool` |
| `string`、`str`、`std::string` | `std::string` |
| `vec<T>`、`std::vector<T>` | `std::vector<T>` |
| `Json`、`cc::Json` | `cc::Json` |

### `where` に書けるもの

リテラル、`self`、`len()`、算術、比較、論理だけです（後で SMT ソルバーに渡せる範囲に絞っています）。連鎖比較（`1 <= self <= 9`）が書けます。それ以外の条件は `matches`（`bool f(const T&)` の形の関数）に書きます。

### 値の作り方

```cpp
Port a = 8080;                   // リテラル：コンパイル時に検査する
Port b = 0;                      // ✗ コンパイルエラー
Port c = n + 1;                  // ✗ コンパイルエラー：コンパイル時に分からない
auto d = Port::try_from(n + 1);  // 実行時に検査する。std::expected<Port, cc::RefineError>
```

- リテラルから作れるのは整数、小数、`bool` を元にした型で、`matches` のないものだけです。文字列を元にした型は `try_from` で作ります
- `try_from` は、受け取った整数が元の型の範囲に収まるかを、変換する前に調べます。`Port::try_from(73616)` は（`u16` にすると 8080 になりますが）失敗します
- 小数を整数の型の `try_from` に渡すとコンパイルエラーです

### 値の使い方

元の型へは暗黙に変換できるので、普通の値として読めます。`get()` でも取り出せます。`==` はリテラルとも比べられ、`<<` で出力できます。

```cpp
Port p = 8080;
int n = p + 1;
if (p == 8080) std::cout << p;
```

## 4. スキーマ：`schema`

```cpp
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

末尾の `;` はあってもなくても構いません。

### フィールド

```
名前: 型 [where 条件] [matches 関数] [transform 関数] [= 既定値];
```

| 要素 | 意味 |
|---|---|
| 型 | `type` の元の型、`T?`（または `std::optional<T>`）、ほかのスキーマや `type`、`enum`、`enum { a, b }` |
| `where`、`matches` | 条件。フィールド専用の入れ子のクラス（`User::id_t`）になる |
| `transform` | 検査の前に値を変換する関数（`T f(T)`）。スキーマ型には使えない |
| `= 既定値` | 値がないときに使う。`[]`、`[a, b]`、リテラル、`enum` の値など。条件も検査される |

`T?` のフィールドは、なければ `std::nullopt` になります。JSON の `null` は「ない」と同じ扱いです。

`enum { a, b }` は `<スキーマ名>_<フィールド名>` という名前の `enum class` になります。

### 生成されるもの

| 生成物 | 内容 |
|---|---|
| `struct User` | フィールドを持つ構造体 |
| `User::parse(const cc::Json&)`、`User::parse(const cc::Raw<cc::Json>&)` | `std::expected<User, cc::ParseError>` を返す |
| `User::make({...})` | プログラムの中の値から作る。`parse` と同じ検査を通す |
| `to_json()` | `cc::Json` への変換 |

```cpp
auto u = User::make({.id = 1, .name = " Alice ", .email = "a@example.com"});
```

`make` の引数は指示付き初期化で、宣言の順に書きます。書かなかったフィールドは既定値が使われ、既定値がなければ必須フィールドのエラーになります。

### エラー

`cc::ParseError` は、壊れたフィールドを**全件**、パス付きで持ちます（`upstreams[1].port` など）。宣言の順に並び、未知のフィールドは最後に来ます。`to_string()` で文章にできます。

| 種類 | 文面 |
|---|---|
| 必須フィールドがない | `必須フィールドがありません` |
| 型が違う | `整数が必要です` など |
| 条件を満たさない | `1 <= self を満たしていません` |
| スキーマにないフィールド | `未知のフィールドです`。無視するなら `open schema` と宣言する |
| `invariant` | 説明の文字列と条件。すべてのフィールドが正しいときだけ検査する |
| JSON でない | `(全体): JSON として解釈できません` |

### `invariant`

```
invariant(条件 [, "説明"]);
```

`invariant` の式は C++ の一部です。比較（連鎖も可）、算術、論理、関数の呼び出し、`.`、`[]`、`::` が書けます。フィールドは名前で、`enum` の値は `admin` のように書けます。`none` は `std::nullopt` です。呼び出す関数はスキーマより前に宣言します。

**`invariant` を持つスキーマのフィールドは `const` です。** `u.age = 3;` はコンパイルエラーで、外から作る手段は `parse` と `make` だけです。値全体の代入（`a = b;`）はできます。

`invariant` のないスキーマは普通の構造体です。`Plain p{5, "text"};` のように作れ、フィールドにも代入できます。条件付きのフィールドは、自分の型が値を守ります。

### `enum`

スキーマから参照された `enum` と `enum class` は、宣言はそのままで、JSON の文字列との対応が足されます。値（`a = 5`）を指定した `enum` はエラーです。同じファイルで宣言された `enum` だけが対象で、ほかのものは次の関数を自分で書きます。

```cpp
inline std::vector<std::string_view> cc_enum_names(Role) { return {"admin", "member"}; }
```

### `cc::Raw<T>`：信頼境界

`cc::parse_json` は `cc::Raw<cc::Json>` を返します。`Raw` のままでは検証済みの型として使えず、C++ のコンパイルエラーになります。

```cpp
void handle(cc::Raw<cc::Json> body) {
  User user = cc::must(User::parse(body));   // ここが信頼境界
}
```

## 5. 関数の契約：`pre` と `post`

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

```
pre(条件 [, "説明"])
post([結果の名前:] 条件 [, "説明"])
```

- 条件は C++ の式です（連鎖比較は書けません）
- `old(式)` は `post` の中で使え、関数に入ったときの値です
- `post` から見える値渡しの引数は、呼び出されたときの値です。本体が書き換えても変わりません
- `post` は関数が値を返したときに検査します。例外で抜けたときは検査しません

### 違反の報告

```
contract violation: pre「残高が足りる」
  条件: a.balance >= amt
  責任: 呼び出し側  withdraw.c.c:32  pay() → withdraw()
  契約: withdraw.c.c:13
  値:   a.balance = 200, amt = 500
```

`pre` の違反は呼び出し側、`post` の違反は実装側の責任として報告します。報告の後で `abort` します。`cc::contract_mode` で変えられます。

| 値 | 動作 |
|---|---|
| `cc::Mode::enforce`（既定） | 報告して止まる |
| `cc::Mode::observe` | 報告して続ける。`-DCC_CONTRACTS_OBSERVE` でも選べる |
| `cc::Mode::off` | 報告しない。`-DCC_CONTRACTS_OFF` でも選べる。条件の式は評価される |

### 書ける場所

名前空間のスコープにある、普通の関数の**定義**です。次はエラーになります。

| 書いたもの | 理由 |
|---|---|
| `template`、メンバ関数、演算子、`requires` | まだ対応していない |
| 可変長引数、`consteval` | まだ対応していない |
| 宣言だけの関数（本体がない） | 定義に書く |
| 名前のない引数 | 外側の関数から渡せない |
| `auto` の戻り値（後置の型なし） | 型を書く |
| `main` | 呼び出し側がいない |

### 生成されるもの

関数は「元の本体」（`cc_impl_withdraw`）と「検査して呼び出す外側の関数」（`withdraw`）に分かれます。呼び出し位置を受け取る隠し引数が末尾に付くので、**関数ポインタの型は変わります。** 同じ引数の宣言が先にあると、呼び出しが曖昧になってコンパイルエラーになります。

## 6. 効果：`[[cc::effects]]`

```cpp
[[cc::effects()]]                 // 純粋
int add(int a, int b) { return a + b; }

[[cc::effects(io, throw)]]
std::string load(const std::string& path);
```

**呼び出す関数の効果は、呼び出し元が宣言した効果に含まれていなければなりません。** 検査するのは `ccc check` だけで、Clang（19 以降）を使います。`ccc build` と `ccc run` は検査しません。

| 効果 | 意味 |
|---|---|
| `io` | ファイル、ネットワーク、標準入出力。`alloc` を含む |
| `alloc` | ヒープ確保。`std::bad_alloc` は `throw` に数えない |
| `throw` | 例外の送出 |
| `async` | コルーチンによる中断 |
| `unsafe` | 生ポインタ、未定義動作の可能性、呼び先の分からない呼び出し |
| `nondet` | 時刻、乱数 |
| `log` | ログ出力 |

### 規則

`[[cc::effects(...)]]` を付けた関数の本体だけを検査します。**注釈のない関数は普通の C++ で、何でもできます。** 注釈は宣言に付ければ定義にも効きます。テンプレートは実体ごとに、ラムダの本体は書いた関数の一部として検査します。`pre` / `post` の条件も関数の一部です。

| 呼び先 | 必要な効果 |
|---|---|
| `[[cc::effects(...)]]` のある関数 | 宣言されたもの |
| 効果の一覧（[effect_table.rs](../compiler/src/effect_table.rs)）にある関数 | 一覧のもの |
| 生成された関数と `cc::` の関数 | ライブラリの宣言。`parse`、`make`、`to_json` は `alloc`、`try_from` は何も要らない |
| 注釈のない関数、一覧にない関数 | `unsafe` |
| 自明なコンストラクタとデストラクタ | 何も要らない |
| 仮想関数、関数ポインタ、`std::function` | `unsafe` |
| `throw` 式 | `throw`。`catch (...)` のある `try` の中なら不要 |
| `new`、`delete` | `alloc` |
| `reinterpret_cast`、値を読み替える C キャスト、インラインアセンブリ | `unsafe` |
| `co_await`、`co_yield`、`co_return` | `async` |

コンパイル時に評価されるもの（`constexpr` の変数、`consteval` の呼び出し）は数えません。

### ハンドラ

`cc::read_file` は差し替えられます。テストのときに便利です。`fopen` の直接呼び出しは差し替わりません（効果の検査では `unsafe` として見つかります）。

```cpp
cc::MemoryFs fs{{"app.json", R"({"port": 8080})"}};
cc::WithIo io{fs};            // このスコープの間だけ
```

## 7. 厳格モード

既定では C++ の挙動を変えません。`#pragma cc strict` と書くと、それより後で次が有効になります。

| 検査 | 実現方法 |
|---|---|
| 値が変わる暗黙の変換、符号付きと符号なしの比較 | `-Wconversion`、`-Wsign-conversion`、`-Wsign-compare`、`-Woverflow` をエラーにする |
| `return` のない経路 | `-Wreturn-type` をエラーにする |
| `v[i]` の範囲外 | `_GLIBCXX_ASSERTIONS` を定義する。ファイル全体に効く |

`#pragma cc strict` の前にインクルードしたヘッダーのコードには、警告の扱いは適用されません。生成されたコードにも適用されません。

`ccc run` と `ccc test` は厳格モードに関係なく UBSan を有効にしてビルドし、オーバーフローやゼロ除算で止まります。

## 8. テスト：`test`

```cpp
test "設定を読める" {
  cc::MemoryFs fs{{"app.json", R"({"port": 8080})"}};
  cc::WithIo io{fs};
  expect(load_config().port == 8080);
}
```

- ファイルのトップレベルに書きます。中身は普通の C++ です
- `expect(条件);` は、外れると条件と値を表示して、続きを実行します。テストは失敗になります
- テストごとに別プロセスで動くので、契約違反で止まっても残りのテストは実行されます
- 例外が漏れるとテストの失敗として報告します
- `ccc run` と `ccc build` の出力をそのままビルドした場合、テストは含まれません（`-DCC_TEST` を付けたときだけ）

`ccc test` のとき、テストはファイルの最後の静的初期化の中で実行されるので、`main` は呼ばれません。

## 9. 拡張付きヘッダー：`.c.h`

```cpp
#include "shared.c.h"
```

`.c.h` は `.c.c` と同じように拡張を書けるヘッダーです。`ccc` が `.h` に変換します。

- `#include "...c.h"` の形だけが対象です。パスは含むファイルからの相対で、`<...>` は対象外です
- 実行のたびに変換するので、古い結果が残りません
- 変換先の名前は `shared.h` のようにファイル名だけになります。同じ名前の `.c.h` を一緒に使うとエラーです
- `ccc build` は出力先のディレクトリに `.h` を書きます。`ccc` が作ったものでない同名のファイルがあれば、上書きせずにエラーにします
- `test` と `#pragma cc strict` は `.c.h` には書けません
- 関数の定義を書くときは、C++ と同じく `inline` を付けます。`#pragma once` なども自分で書きます

`ccc build` の出力を別のディレクトリでビルドするときは、元のディレクトリを `-iquote` で渡すと、普通の `#include "x.h"` も見つかります。`ccc run` などは自動で渡します。

## 10. `<cc/cc.hpp>` のライブラリ

拡張の外では `cc::` を付けて使います。

| 名前 | 内容 |
|---|---|
| `cc::i8` 〜 `cc::u64`、`cc::f64`、`cc::string`、`cc::vec<T>` | 型の別名 |
| `cc::must(r)` | `std::expected` の値を取り出す。失敗なら `cc::Failure` を送出する |
| `cc::parse_json(text)` | `cc::Raw<cc::Json>` を返す |
| `cc::read_file(path)` | ファイルを読む。失敗なら `cc::Failure`。差し替え可 |
| `cc::print(...)`、`cc::eprint(...)` | 空白で区切って標準出力、標準エラーへ。`expected`、`optional`、`vector`、スキーマも表示する |
| `cc::log(...)` | `[log]` を付けて標準エラーへ |
| `cc::len(x)`、`cc::trim(s)`、`cc::email_format(s)` | `where`、`transform`、`matches` で使う関数 |
| `cc::MemoryFs`、`cc::WithIo` | ハンドラ |
| `cc::repr(x)`、`cc::display(x)` | 値を文字列にする |
| `cc::Raw<T>`、`cc::Json`、`cc::ParseError`、`cc::RefineError`、`cc::Failure` | 型 |
| `cc::contract_mode` | 契約違反の動作 |

`CC_PRE`、`CC_POST`、`CC_V` は、`ccc` を使わずに同じ報告を出すマクロです（[examples/hand/withdraw.cpp](../examples/hand/withdraw.cpp)）。

## 11. つまずきやすい点

**`Port p = n;` がコンパイルできない。** リテラル以外は、条件を満たすかコンパイル時に分かりません。`Port::try_from(n)` を使います。

**文字列を元にした型にリテラルを代入できない。** `Name n = "a";` はコンパイルエラーです。`Name::try_from("a")` を使います。

**`invariant` のあるスキーマのフィールドに代入できない。** 仕様です。`make` で作り直して値全体を代入します。

**`invariant` から呼ぶ関数が見つからない。** スキーマより前に宣言します。

**`effects()` の関数で `std::string` を作るとエラーになる。** 確保は `alloc` です。`std::string_view` を使うか、`effects(alloc)` と宣言します。

**`effects()` の関数から注釈のない関数を呼ぶとエラーになる。** `unsafe` 扱いです。呼ぶ関数に注釈を付けます。

**`ccc run` で効果のエラーが出ない。** 効果を検査するのは `ccc check` だけです。

**関数ポインタに入れられない。** `pre` / `post` のある関数は隠し引数を持ちます。ラムダで包みます。

**`pre` が拡張として扱われない。** 同じファイルに `#define pre` があるか、関数の本体の中です。

**`v[i]` が範囲外でも止まらない。** `#pragma cc strict` を書きます。

## 12. `ccc` のコマンドと終了コード

```sh
ccc run   file.c.c [-- 引数...]
ccc test  file.c.c
ccc check file.c.c
ccc build file.c.c [-o out.cpp]
```

| 状況 | 終了コード |
|---|---|
| 成功 | 0 |
| `ccc` のエラー、C++ コンパイラのエラー、効果のエラー | 1 |
| `ccc run` のプログラムの終了 | プログラムの終了コード |
| シグナルでの終了 | 128 + シグナル番号。契約違反は 134 |
| `ccc test` | すべて成功なら 0、失敗があれば 1 |

C++ コンパイラのエラーは、`.c.c` の行と `error:` の文だけを表示します。候補の一覧などを含む全文は `CC_VERBOSE=1` で見られます。

## 13. 文法

C++ の部分は C++ の文法のとおりです。拡張だけを示します。

```
type-decl    = "type" name "=" field-type { "where" expr | "matches" name } ";"

schema-decl  = [ "open" ] "schema" name "{" { field | invariant } "}" [ ";" ]
field        = name ":" field-type { "where" expr | "matches" name | "transform" name | "=" expr } ";"
invariant    = "invariant" "(" expr [ "," string ] ")" ";"
field-type   = path [ "<" field-type ">" ] { "?" }
             | "enum" "{" name { "," name } [ "," ] "}"
path         = name { "::" name }

contract     = { "pre" "(" c++-expr [ "," string ] ")"
               | "post" "(" [ name ":" ] c++-expr [ "," string ] ")" }

test-decl    = "test" string c++-compound-statement

effects-attr = "cc" "::" "effects" "(" [ effect { "," effect } ] ")"     (* [[ ]] の中 *)

expr         = or
or           = and { "||" and }
and          = compare { "&&" compare }
compare      = add { ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) add }
add          = mul { ( "+" | "-" ) mul }
mul          = unary { ( "*" | "/" | "%" ) unary }
unary        = ( "-" | "!" ) unary | postfix
postfix      = primary { "(" args ")" | "." name | "[" expr "]" }
primary      = number | string | "true" | "false" | "none" | "self"
             | path | "[" args "]" | "(" expr ")"
```

`where` では、`primary` のうちリテラルと `self`、呼び出しのうち `len(...)` だけが使えます。
