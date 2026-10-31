# C.C. 言語仕様（試作版）

今の `ccc` が実際に受け付ける言語を定めた文書です。将来の構想は [CONCEPT.md](../CONCEPT.md) にあり、ここには書きません。

- 対象の環境は Ubuntu 24.04、g++ 13 です
- 仕様の多くは、実際にプログラムを動かして確かめています。文書と実装が食い違っていたら、今の正解は実装です
- 互換性は保証しません。構文も挙動も変わる可能性があります

**先に読むとよい節:** [最初のプログラム](#1-最初のプログラム)、[つまずきやすい点](#16-つまずきやすい点)

## 目次

1. [最初のプログラム](#1-最初のプログラム)
2. [ファイルの構成](#2-ファイルの構成)
3. [字句](#3-字句)
4. [型](#4-型)
5. [宣言](#5-宣言)
6. [文](#6-文)
7. [式](#7-式)
8. [値の契約：条件付きの型](#8-値の契約条件付きの型)
9. [スキーマと JSON](#9-スキーマと-json)
10. [関数の契約](#10-関数の契約)
11. [効果](#11-効果)
12. [失敗の扱い](#12-失敗の扱い)
13. [組み込み関数と表示形式](#13-組み込み関数と表示形式)
14. [テスト](#14-テスト)
15. [C++ との連携](#15-c-との連携)
16. [つまずきやすい点](#16-つまずきやすい点)
17. [コマンドと実行時の挙動](#17-コマンドと実行時の挙動)
18. [文法](#18-文法)

## 1. 最初のプログラム

```cpp
type Port = u16 where 1 <= self <= 65535;

schema Server {
  name: string where 1 <= len(self) <= 50 transform trim;
  port: Port;
}

contract fn half(n: i64) -> i64
  effects()
  pre(n >= 0, "負でない")
  post(r: r * 2 <= n)
{
  return n / 2;
}

fn main() -> i32 effects(io) {
  match Server::parse(parse_json(R"({"name": " web ", "port": 8080})")) {
    ok(s)  { print(s.name, s.port); }
    err(e) { eprint(e); }
  }
  print(half(10));
  return 0;
}

test "half" {
  expect(half(10) == 5);
}
```

```sh
ccc run hello.c.c    # web 8080 と 5 を表示する
ccc test hello.c.c   # test ブロックを実行する
```

## 2. ファイルの構成

- 1つのプログラムは1つの `.c.c` ファイルです。`import` やモジュールはありません
- トップレベルに書けるのは宣言だけです。グローバル変数はありません
- `type`、`schema`、`enum`、`cpp` は**ソースの順に**有効になるので、使う前に宣言します。`schema A { b: B; }` の後に `schema B` を書くとエラーです
- 関数は順序に関係なく呼べます。相互再帰もできます

## 3. 字句

| 要素 | 規則 |
|---|---|
| コメント | `// 行末まで`、`/* 複数行 */` |
| 名前 | ASCII の英字か `_` で始まり、英数字と `_` が続く。**日本語の名前は使えない** |
| 整数 | 10進数のみ。`1_000_000` と書ける。16進数（`0x10`）はない |
| 小数 | `2.5`、`1e3`、`1.5e-1` |
| 真偽値 | `true`、`false` |
| 文字列 | `"..."`。UTF-8 可、途中で改行できない。エスケープは `\n` `\t` `\r` `\0` `\\` `\"` `\'` だけ |
| 生文字列 | `R"(...)"`、`R"x(...)x"`。複数行も可 |

予約語の扱いは次のとおりです。

- `true`、`false`、`none`、`self` は常にその意味になります
- `cpp` の直後に `{` があると C++ ブロックになります
- それ以外のキーワードは場所によって意味が決まります。C++ のキーワードと同じ名前（`class`、`default` など）も変数や関数に使えます
- `break`、`continue` は構文にありません（[6章](#6-文)）

## 4. 型

| 型 | 内容 | C++ |
|---|---|---|
| `i32` `i64` | 符号付き整数。`int` は `i64` の別名 | `int32_t` `int64_t` |
| `u8` `u16` `u32` `u64` | 符号なし整数 | `uint8_t` など |
| `f64` | 小数 | `double` |
| `bool` | 真偽値 | `bool` |
| `string` | 文字列。`str` は別名 | `std::string` |
| `vec<T>` | 配列 | `std::vector<T>` |
| `T?` | 省略可能な値。`none` を取れる | `std::optional<T>` |
| `Json` | 任意の JSON | `nlohmann::json` |
| `Raw<T>` | 未検証のデータ。[9章](#9-スキーマと-json) | `cc::Raw<T>` |
| 名前 | `type`、`schema`、`enum` の型、または C++ の型 | 同名の型 |

`T&` は参照で、**関数の引数にだけ**書けます。戻り値やローカル変数には使えません。

`T::parse` と `T::try_from` の結果の型（成功か失敗か）は、**ソースに書けません**。`let r = T::parse(j);` で受けるか、その場で `match` か `?` を使います。

### 整数の扱い

- `let x = 0;` のように右辺が整数リテラルだけの式なら、`x` は `i64` です。`[1, 2, 3]` は `vec<i64>` です
- それ以外の演算結果の型は C++ の規則に従います。`u8 + u8` は 32 ビットの `int` になります

### 暗黙の変換

値が変わらない変換だけが暗黙に行われます。次はコンパイルエラーです。

| 例 | 理由 |
|---|---|
| `let a: u16 = n;`（`n` は `i64`） | 値が変わりうる |
| `let a: u64 = n;`（`n` は `i64`） | 符号が変わりうる |
| `let n: i64 = d;`（`d` は `f64`）、`let f: f64 = n;`（`n` は `i64`） | 値が変わりうる |
| `a < b`（`a` は `i64`、`b` は `u64`） | 符号付きと符号なしの比較 |
| `let a: u16 = 70000;` | リテラルが範囲外 |

`let a: u16 = 5;`、`let b: i64 = a;`、`let f: f64 = 5;` は通ります。

**型を変換する構文はありません。** 整数の幅を変えるときは、条件付きの型の `try_from` を通します（[8章](#8-値の契約条件付きの型)）。`i64` から `f64` へ変換する手段は、`cpp { }` か `extern fn` だけです。

## 5. 宣言

```cpp
type Port  = u16 where 1 <= self <= 65535;     // 条件付きの型
enum Role { admin, member }                    // 列挙
schema User { id: u64; }                       // スキーマ
open schema Event { id: u64; }                 // 未知のフィールドを許すスキーマ
fn add(a: i64, b: i64) -> i64 effects() { return a + b; }
contract fn pos(a: i64) -> i64 effects() pre(a > 0) { return a; }
extern fn sqrt2(a: f64) -> f64 effects();      // C++ の関数
cpp { /* C++ */ }
test "名前" { expect(1 + 1 == 2); }
```

### 関数

- 引数は `名前: 型` の順で書きます。`T&` は参照渡しで、それ以外はコピーです（スキーマ型もコピーされます）
- `-> 型` を省略すると値を返しません
- `effects`、`pre`、`post` の順序は自由です
- 同じ名前は1つだけ宣言できます。オーバーロードはありません
- 組み込み関数の名前と基本型の名前は宣言に使えません
- 戻り値の型を宣言した関数に、`return` せずに終わる経路があるとコンパイルエラーです。`match` の両方の分岐で `return` するのは問題ありません
- 戻り値を無視してもエラーになりません
- 本体のない `fn f(...);` は、`extern fn f(...);` と同じ意味になります

### `main`

- 省略できます。ない場合、`ccc test` は動きますが、`ccc run` はリンクエラーになります
- 引数はなしか `args: vec<string>` だけです。`args` にプログラム名は含まれません
- 戻り値は、なし、`i32`、`i64` のいずれかです。値は終了コードになり、256 で割った余りが使われます
- 戻り値の型を省略しても `return 3;` と書けます

### `enum`

```cpp
enum Role { admin, member }

fn f(r: Role) -> Role effects() {
  if r == admin { return member; }
  return Role::admin;
}
```

- `Role::admin` はどこでも書けます。`admin` だけで書けるのは、型が分かる場所（比較の相手、型付きの `let`、引数、`return`、既定値）です。`let t = admin;` はエラーです
- `match` では分岐できません。`if` で比較します
- `print` すると名前が表示されます
- 整数との変換はできません
- スキーマのフィールドには `role: enum { a, b } = a;` と直接書けます

## 6. 文

| 文 | 例 | 補足 |
|---|---|---|
| `let` | `let x = 1;`、`let x: i64 = 1;` | 初期値は必須。**すべての変数は書き換えられる** |
| 代入 | `x = 1;` `+=` `-=` `*=` `/=` `%=` | 式としては使えない |
| 式文 | `f(x);` | |
| `return` | `return;`、`return x;` | |
| `if` | `if c { } else if d { } else { }` | 条件の `( )` は不要。`{ }` は必須 |
| `while` | `while c { }` | |
| `for`（範囲） | `for i in 0..n { }` | `i` は `i64`。`n` は含まない。`n` は毎回評価される |
| `for`（要素） | `for x in xs { }` | `x` は要素への**参照** |
| ブロック | `{ }` | スコープを作る |
| `match` | [12章](#12-失敗の扱い) | 式としては使えない |
| `try` / `catch` | [12章](#12-失敗の扱い) | |
| `cpp` | `cpp { ... }` | [15章](#15-c-との連携) |
| `expect`、`with` | [14章](#14-テスト) | `test` の中だけ |

**ない構文:** `break`、`continue`、`++`、`--`、C 形式の `for`、初期値なしの `let`。ループを途中で抜けるには、条件で制御するか、関数に分けて `return` します。

### スコープ

- ブロックごとにスコープができます。内側で同じ名前を宣言して隠すのは問題ありません
- 同じスコープでの再宣言はエラーです。関数の一番外側で、引数と同じ名前を `let` するのもエラーです

## 7. 式

### 演算子（優先順位の低い順）

| 順位 | 演算子 | 補足 |
|---|---|---|
| 1 | `\|\|` | |
| 2 | `&&` | |
| 3 | `==` `!=` `<` `<=` `>` `>=` | 連鎖できる |
| 4 | `+` `-` | |
| 5 | `*` `/` `%` | 整数の `/` は 0 方向に切り捨て |
| 6 | `-x` `!x` | 前置 |
| 7 | `f(x)` `a.b` `a[i]` `x?` | 後置 |

**連鎖する比較:** `1 <= x <= 9` は `1 <= x && x <= 9` と同じです。真ん中の式は、左の比較が真のとき**2回評価**されます。

**ない演算子:** ビット演算（`|` `&` `^` `~` `<<` `>>`）、三項演算子、キャスト。

### その他の式

| 式 | 補足 |
|---|---|
| `[a, b]` | 配列。`[]` は型が分かる場所でだけ書ける（`let v: vec<i64> = [];`） |
| `none` | `T?` の値なし |
| `a[i]` | `vec`、`string`、`Json` に使える。範囲外は契約違反で停止（[10章](#10-関数の契約)） |
| `T::parse(j)`、`T::try_from(v)` | 生成された関数 |
| `Role::admin` | `enum` の値 |
| `s + t`、`s == t`、`s < t` | 文字列の連結と比較。`"a" + 1` はエラー。数値を文字列に変換する関数はない |
| `s[i]` | 1 **バイト**を返す |
| `x == none`、`x == 5` | `T?` の比較。`x + 1` は不可で、`match` で取り出す |

条件の式に `bool` 以外を渡しても、C++ の規則で通ります（`if 1 { }`）。

## 8. 値の契約：条件付きの型

```cpp
type Port  = u16 where 1 <= self <= 65535;
type Name  = string where 1 <= len(self) <= 50;
type Email = string matches email_format;
type Ratio = f64 where 0.0 <= self <= 1.0;
type Tags  = vec<string> where len(self) <= 3;
```

- 元にできるのは、基本型、`Json`、`vec<T>` です
- `where` か `matches` のどちらかが必須で、両方も書けます

### `where` に書けるもの

リテラル、`self`、`len()`、算術、比較、論理、前置の `-` と `!` だけです。ほかの変数や関数は書けません。それ以外の条件は `matches` か、スキーマの `invariant` に書きます。

`matches` に使えるのは、純粋な組み込み関数（`email_format` など）と、`effects()` を宣言した `extern fn` です。**C.C. で書いた関数は使えません。**

### 値の作り方

| 書き方 | 検査 | 結果 |
|---|---|---|
| `let p: Port = 8080;`（整数か真偽値のリテラル） | コンパイル時 | 条件を満たさないとコンパイルエラー |
| `f(8080)`、`return 8080;`（リテラル） | コンパイル時 | 同上 |
| `Port::try_from(n)` | 実行時 | 成功か失敗 |
| `let w: Port = q;`（`q` も `Port`） | なし | コピー |

次は認められません。

| 例 | 結果 |
|---|---|
| `let n: Name = "abc";`、`let r: Ratio = 0.5;` | 文字列と小数のリテラルは静的に検査できないのでエラー。`try_from` を使う |
| `let p: Port = n;`（`n` は `i64`） | エラー。`try_from` を使う |
| `q = 90;`（代入） | リテラルでもエラー。`q = p;` は可 |
| `let v: vec<Port> = [80];`、`let p: Port? = 80;` | エラー |

### `try_from` の引数

| 元の型 | 受け取れるもの |
|---|---|
| 整数 | 任意の整数と、整数を元にした条件付きの型。**先に範囲を調べる**（`Port::try_from(73616)` は失敗。8080 に切り詰められない）。小数を渡すとコンパイルエラー |
| `bool` | 真偽値。整数を渡すとコンパイルエラー |
| `f64` | 小数と整数 |
| `string` | 文字列 |
| `vec<T>` | 配列 |

### 値の使い方

元の型に暗黙に変換されるので、普通の値として使えます。`p + 1`、`p == 80`、`p < q`、`let n: i64 = p;`、`len(name)`、`for x in tags`、`tags[0]`、`p.get()` が書けます。演算の結果は元の型の値で、条件はもう保証されません。

## 9. スキーマと JSON

```cpp
schema User {
  id:    u64         where self > 0;
  name:  string      where 1 <= len(self) <= 50 transform trim;
  email: Email;
  role:  enum { admin, member } = member;
  age:   int?        where 0 <= self <= 150;
  tags:  vec<string> where len(self) <= 3 = [];

  invariant(role != admin || age >= 18, "管理者は18歳以上");
}
```

スキーマ1つから、構造体、`User::parse()`、`to_json()` が生成されます。**値を作る方法は `parse` だけです**（ほかは `cpp { }` か `extern fn`）。`User { id: 1 }` のような構文はありません。

### フィールド

`名前: 型` の後に、次を任意の順で書けます。

| 句 | 意味 |
|---|---|
| `where 式` | 条件。`self` はフィールドの値。`T?` なら中身の値 |
| `matches f` | 述語関数による条件 |
| `transform f` | 検査の前に値を変換する。組み込みでは `trim` が使える |
| `= 式` | 既定値。キーがないか `null` のときに使う |

### `parse` の処理順

1. 入力が JSON オブジェクトか確かめます。解釈できない JSON も、ここでエラーになります
2. フィールドを宣言順に処理します。各フィールドは、デコード → `transform` → `where` / `matches` の順です
3. スキーマにないキーをエラーにします（`open schema` なら無視します）
4. ここまでにエラーがなければ、すべての `invariant` を評価します

エラーは止まらずに**全件**集めます。パスは `addrs[1].city` のように示され、スキーマ全体のエラーは `(全体)` と表示されます。

### JSON からの変換

| 型 | 受け付ける値 |
|---|---|
| 整数 | JSON の整数で、型の範囲に収まるもの。`1.0` や `1.5` はエラー |
| `f64` | 数値 |
| `bool` | `true` / `false` のみ。`1` はエラー |
| `string` | 文字列 |
| `enum` | 値の名前と完全に一致する文字列 |
| `vec<T>` | 配列 |
| `T?` | 中身の値、または `null` か省略 |
| スキーマ | オブジェクト |
| `Json` | 何でも。ただし `null` は省略扱いなので、`Json?` と書く |

### `invariant`

- `invariant(条件);` または `invariant(条件, "説明");` と書きます
- フィールドは名前だけで参照します。`effects()` を宣言した C.C. の関数を呼べます（宣言の順序は問いません）
- `T?` のフィールドは比較できます。値がないときの結果は `std::optional` の規則に従います。`age >= 18` は false、`age < 18` は **true** です。`age == none || ...` のように明示するのが安全です

### 既定値

- 純粋でなければなりません
- `where` / `matches` は既定値にも適用されます。満たさない場合は `parse` のエラーとして報告されます
- 名前付きの整数の型（`p: Port = 0;`）のリテラルは、コンパイル時に検査されます
- `transform` は既定値には適用されません

### `to_json`

`s.to_json()` は `Json` を返します。キーはアルファベット順で、値のない `T?` は `null` になります。

### `Raw<T>`：信頼境界

`parse_json(text)` は `Raw<Json>` を返します。これは検証前のデータで、スキーマ型や `Json` として渡すとコンパイルエラーです。

```cpp
fn handle(body: Raw<Json>) effects(io, throw) {
  let user = User::parse(body)?;   // ここで検証する
  greet(user);
  // greet(body);                  // エラー: 未検証の Raw<Json> を User として使えません
}
```

- `Raw<T>` は引数と `let` に使えます。スキーマのフィールドには書けません
- `parse` は `Json` も受け取ります
- `Json` として読むには `.unchecked()` を呼びます。C++ のメソッドなので `unsafe` が必要です

`Json` は `j["a"]`、`j["a"][1]` で読めます。ないキーは `null` になり、範囲外の添字は停止します。

## 10. 関数の契約

```cpp
contract fn withdraw(a: Account&, amt: i64) -> i64
  effects(log)
  pre(amt > 0, "金額は正")
  pre(a.balance >= amt, "残高が足りる")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt, "残高が amt だけ減る")
{
  a.balance -= amt;
  log("withdraw", amt);
  return amt;
}
```

- `pre` / `post` は `contract fn` にだけ書けます。どちらも書かない `contract fn` も認められます
- 説明の文字列は省略できます。省略すると条件の式が表示されます
- `pre` は関数の先頭で、書いた順に評価されます
- `post` は `return` のたびに（戻り値なしなら本体の末尾でも）、書いた順に評価されます。例外で抜けるときは評価されません
- `post(r: ...)` の `r` は戻り値です。名前は自由です
- `old(式)` は `post` の中だけで使えます。関数の入口で値をコピーします
- `post` で引数を読むと、**書き換えた後の値**になります。入口の値は `old()` で読みます
- 条件は純粋でなければなりません（[11章](#11-効果)）

### 違反の報告

| 違反 | 責任 | 報告の位置 |
|---|---|---|
| `pre` | 呼び出し側 | 呼び出した行と、契約の行 |
| `post` | 実装側 | 契約の行と、呼び出した行 |
| `v[i]` の範囲外 | 呼び出し側 | 添字を使った行 |

```
contract violation: pre「残高が足りる」
  条件: a.balance >= amt
  責任: 呼び出し側  withdraw.c.c:35  pay() → withdraw()
  契約: withdraw.c.c:11
  値:   a.balance = 200, amt = 500
```

### モード

| モード | 指定 | 挙動 |
|---|---|---|
| enforce（既定） | | 報告して停止する |
| observe | `-DCC_CONTRACTS_OBSERVE` または `cc::contract_mode = cc::Mode::observe;` | 報告して続行する |
| off | `-DCC_CONTRACTS_OFF` | 条件は評価するが、報告しない |

`ccc run` には C++ のオプションを渡せません。`-D` を使うには `ccc build` の後に自分で g++ を実行します。`v[i]` の範囲外は、どのモードでも停止します。

## 11. 効果

```cpp
fn pure_add(a: i64, b: i64) -> i64 effects() { return a + b; }
fn load(p: string) -> string effects(io, throw) { return read_file(p); }
```

**規則:** 関数の中で呼ぶものの効果は、その関数が宣言した効果に含まれていなければなりません。違反はコンパイルエラーです。

| 効果 | 必要になるもの |
|---|---|
| `io` | `print`、`eprint`、`read_file` |
| `throw` | `?`、`read_file` |
| `log` | `log` |
| `unsafe` | 効果が分からないもの（下の表） |
| `alloc` `async` `nondet` | 組み込み機能では使わない。宣言すると呼び出し元に伝わる |

`unsafe` が必要になるものは次のとおりです。

| 呼び出すもの | 例 |
|---|---|
| `effects(...)` を省略した関数 | |
| 宣言していない C++ の関数 | `foo(1)`、`Foo::bar(1)` |
| C++ のメソッド | `v.push_back(1)`、`s.size()` |
| 関数の中の `cpp { }` | |

例外として、型が分かっているスキーマ値の `to_json()` と、条件付きの型の `get()` は純粋です。

### 補足

- **`effects(...)` を省略した関数は検査されず、何でもできます。** ただし、効果を宣言した関数から呼ぶには `unsafe` が必要です。`test` ブロックも検査されません
- `throw` を宣言していない関数は `noexcept` になります。`unsafe` な C++ から例外が出ると、プログラムは異常終了します
- `try` の中の `throw` は外に伝わりません（[12章](#12-失敗の扱い)）
- `where`、`pre`、`post`、`invariant`、既定値は純粋でなければなりません
- 未知の効果名はエラーです
- `v[i]` の範囲外や整数オーバーフローは、効果として追跡しません
- `extern fn` の効果の宣言は信用するだけで、検証しません

## 12. 失敗の扱い

`T::parse` と `T::try_from` の結果は、成功か失敗のどちらかです。そのまま `T` として渡すとエラーになります。

### `match`

```cpp
match User::parse(body) {       // ok / err の両方が必須
  ok(user) { greet(user); }
  err(e)   { eprint(e); }
}

match user.age {                // T? は some / none の両方が必須
  some(age) { print(age); }
  none      { print("年齢なし"); }
}
```

- 分岐できるのは、この2種類だけです。分岐の漏れや重複はエラーです
- `err(e)` の `e` は `print` / `eprint` で表示します

### `?`

```cpp
let user = User::parse(body)?;
return Port::try_from(n)?;
Port::try_from(n)?;
```

- 失敗すると例外を送出します。**`throw` の効果が必要です**
- 書けるのは、`let` の右辺、`return` の値、式文の末尾だけです。`print(f()?)` は書けません
- 使えるのは `parse` / `try_from` の結果だけです。`T?` には使えません
- `main` まで届くと、`エラー: メッセージ` を表示して終了コード 1 で終わります

### `try` / `catch`

```cpp
try {
  let text = read_file(path);
} catch message {             // message は string
  eprint(message);
}
```

- `try` の中の `throw` は外に伝わらないので、呼び出し元に `throw` は要りません
- 捕まえるのは `std::exception` の派生だけです。契約違反の停止は捕まえられません
- 自分で例外を送出する構文（`throw 式`）はありません
- `try` の中で宣言した変数は、`catch` からは見えません

## 13. 組み込み関数と表示形式

| 関数 | 効果 | 内容 |
|---|---|---|
| `print(...)` | `io` | 標準出力へ。空白区切りで、最後に改行 |
| `eprint(...)` | `io` | 標準エラーへ |
| `log(...)` | `log` | 標準エラーへ。先頭に `[log]` |
| `len(x) -> i64` | なし | 文字列、配列、条件付きの型の長さ。**文字列はバイト数**（`len("日本語")` は 9） |
| `trim(s) -> string` | なし | 前後の空白（スペース、タブ、改行）を除く |
| `email_format(s) -> bool` | なし | `@` が1つ、ドメインに `.` があるかの簡単な検査 |
| `parse_json(s) -> Raw<Json>` | なし | 例外は出さない。不正な JSON は `parse` のエラーになる |
| `read_file(path) -> string` | `io`、`throw` | 読めないと失敗を送出する |

### `print` の表示

| 値 | 表示 |
|---|---|
| 文字列 | そのまま |
| `true` / `false` | `true` / `false` |
| 整数、小数 | `5`、`1.5`。`3.0` は `3` |
| `vec` | `[1, 2]`。中の文字列は引用符付き（`["a"]`） |
| `T?` | 中身か `none` |
| `enum` | 名前 |
| スキーマ | JSON |
| `parse` / `try_from` の結果 | `ok(...)` / `err(...)` |
| 検証エラー | 件数とエラーの一覧 |

## 14. テスト

```cpp
fn load() -> string effects(io, throw) { return read_file("a.json"); }

test "ファイルを差し替える" {
  with io = MemoryFs{ {"a.json", "{}"} } {
    expect(load() == "{}");
  }
}
```

- `ccc test` で実行します。`ccc run` では無視されます
- 各テストは別のプロセスで動きます（POSIX 専用）。契約違反で止まっても、残りのテストは実行されます
- `expect(条件);` は `test` の中に、文として書きます。外れても続きを実行し、失敗として数えます。条件の中の値も表示します
- 例外が外に出ると、そのテストは失敗です
- 1件でも失敗すると終了コードは 1 です
- 1ファイルずつの実行で、テストの絞り込みはできません

### `with`

- `with io = MemoryFs{ {"パス", "中身"}, ... } { ... }` だけが使えます。`test` の中でのみ書けます
- 差し替わるのは `read_file` だけです。`print` は本物のまま出力されます
- 登録していないパスは、ファイルがないときと同じ失敗になります

### 出力の例

```
test 通る
  ok
test 落ちる
  expect 失敗: x == 2  (tests.c.c:4)
    x = 1
  FAILED
2 件中 1 件成功
```

## 15. C++ との連携

### `cpp { }`

```cpp
cpp {
#include <cmath>
struct Account { std::int64_t balance; };
inline double root(double a) { return std::sqrt(a); }
}
extern fn root(a: f64) -> f64 effects();

fn main() effects(io, unsafe) {
  let i = 3;
  cpp { i = i + 1; }       // 関数の中では unsafe が必要。ローカル変数が見える
  print(i, root(16.0));
}
```

- トップレベルの `cpp` はソースの順に出力され、関数の宣言より前に置かれます。**中から C.C. の関数は呼べません**
- 中身は検査しません。暗黙の変換の禁止なども適用されません

### `extern fn`

- C++ の関数を、効果を付けて宣言します。本体は書けません
- 関数は `cpp { }` の中に定義します。`ccc run` に別のソースやライブラリを渡す方法はありません
- 引数の個数は検査します。効果の宣言は信用します
- `extern contract fn` に書いた **`pre` / `post` は検査されません**（[16章](#16-つまずきやすい点)）

### C++ の型

`let a: Account = ...` のように、宣言していない型名はそのまま C++ に渡ります。`std::string` のような `::` を含む名前は書けないので、`cpp` の中で別名を付けます。

### 生成される C++

`ccc build` で確認できます。

| C.C. | C++ |
|---|---|
| 関数 | `namespace cc_user` の中の関数 |
| `main` | `cc_user::cc_main` と、それを呼ぶ `main` |
| C++ のキーワードと同じ名前 | 末尾に `_`（`class` → `class_`） |
| `a[i]` | `::cc::at(a, i)` |
| `contract fn` | 呼び出し位置を受け取る隠し引数の付いた関数 |

## 16. つまずきやすい点

### 書けないこと

| やりたいこと | 今の書き方 |
|---|---|
| ループを抜ける | `break` はない。`while` の条件か、関数に分けて `return` |
| 構造体の値を作る | `parse` か、`cpp { }` / `extern fn` |
| `vec` に追加する | `v.push_back(x)` と `effects(..., unsafe)` |
| 整数の幅を変える | 条件付きの型の `try_from` |
| 数値を文字列にする | 手段がない。`print` に並べて渡す |
| `parse` の結果を返す関数 | 型が書けない。`?` で `T` にして返す |
| `enum` で分岐する | `if` で比較する |

### 意図と違う動きをしやすいこと

| 挙動 | 説明 |
|---|---|
| `len` はバイト数 | `where len(self) <= 50` は50文字ではなく50バイト |
| `for x in xs` の `x` は参照 | `x = 9;` で配列の中身が変わる |
| 連鎖比較は真ん中を2回評価することがある | `1 < f() < 9` は、`1 < f()` が真なら `f` をもう一度呼ぶ |
| 値のない `T?` の比較 | `age < 18` は true になる |
| `let x = none;` | 型が定まらず、`print` は `<?>` を表示する |

### 契約が効かない場所（既知の穴）

| 挙動 | 説明 |
|---|---|
| **フィールドへの代入は `invariant` を通らない** | `invariant(n > 0)` があっても `s.n = -5;` は通る。`where` のあるフィールドへの代入はコンパイルエラーになる |
| **`extern contract fn` の `pre` / `post`** | エラーにならず、検査もされない |
| `cpp { }` と `extern fn` | 中身は検査できない |
| `ccc build` の出力を UBSan なしでビルドする | オーバーフローとゼロ除算が検出されない |

### エラーメッセージの見分け方

| 形式 | 出どころ |
|---|---|
| `file:行:列: エラー: 日本語の文` と `^^^` | `ccc` 自身の検査 |
| `file:行: エラー: 英語の文` | C++ コンパイラ。型の誤り、未定義の名前など。**行が少しずれることがある**。全文は `CC_VERBOSE=1` |
| `file:行:列: runtime error:` | 実行時の UBSan。列番号は生成コードのものなので、行だけ見る |

## 17. コマンドと実行時の挙動

```sh
ccc run   file.c.c [-- 引数...]    # 変換・ビルドして実行する
ccc test  file.c.c                 # test ブロックを実行する
ccc check file.c.c                 # 実行ファイルは残さずに、C++ のコンパイルまで検査する（test も含む）
ccc build file.c.c [-o out.cpp]    # C++ に変換する。C++ コンパイラは動かさない
```

| 環境変数 | 内容 |
|---|---|
| `CXX` | C++ コンパイラ（既定: `g++`） |
| `CC_HOME` | `runtime/` と `third_party/` のあるディレクトリ（既定: ビルドしたときのリポジトリ） |
| `CC_VERBOSE` | C++ コンパイラの出力を省略せずに表示する |

`ccc run` と `ccc test` は UBSan を有効にしてビルドします。`ccc build` の出力を自分でビルドするときは、次のようにします。

```sh
g++ -std=c++23 -Iruntime -Ithird_party -fsanitize=undefined -fno-sanitize-recover=undefined out.cpp -o out
```

### 実行時に止まる場合

| 原因 | 表示 | 終了コード |
|---|---|---|
| `main` の `return` | | その値（256 の余り） |
| `?` などの例外が `main` まで届く | `エラー: メッセージ` | 1 |
| 整数オーバーフロー、ゼロ除算 | `runtime error: ...` | 1 |
| 契約違反、`v[i]` の範囲外 | `contract violation: ...` | 134 |
| その他のシグナル | `プログラムが異常終了しました（シグナル N）` | 128 + N |

C.C. の検査やビルドの失敗は、終了コード 1 です。

## 18. 文法

`compiler/src/parser.rs` から起こしたものです。`{ x }` は0回以上、`[ x ]` は省略可能を表します。

```ebnf
program     = { item } ;
item        = CPP_BLOCK | type_decl | enum_decl | schema_decl | test_decl | fn_decl ;

type_decl   = "type" NAME "=" type { "where" expr | "matches" NAME } ";" ;
enum_decl   = "enum" NAME "{" [ NAME { "," NAME } [ "," ] ] "}" ;
schema_decl = [ "open" ] "schema" NAME "{" { field | invariant } "}" ;
field       = NAME ":" type { "where" expr | "matches" NAME | "transform" NAME | "=" expr } ";" ;
invariant   = "invariant" "(" expr [ "," STRING ] ")" ";" ;
test_decl   = "test" STRING block ;
fn_decl     = [ "extern" ] [ "contract" ] "fn" NAME "(" [ params ] ")" [ "->" type ]
              { clause } ( ";" | block ) ;
params      = param { "," param } [ "," ] ;
param       = NAME ":" type [ "&" ] ;
clause      = "effects" "(" [ NAME { "," NAME } [ "," ] ] ")"
            | "pre" "(" expr [ "," STRING ] ")"
            | "post" "(" [ NAME ":" ] expr [ "," STRING ] ")" ;

type        = base_type { "?" } ;
base_type   = "vec" "<" type ">" | "Raw" "<" type ">" | "Json"
            | "enum" "{" [ NAME { "," NAME } [ "," ] ] "}"   (* フィールドだけ *)
            | NAME ;                                         (* 基本型を含む *)

block       = "{" { stmt } "}" ;
stmt        = CPP_BLOCK | block
            | "let" NAME [ ":" type ] "=" expr ";"
            | "return" [ expr ] ";"
            | if_stmt
            | "for" NAME "in" expr [ ".." expr ] block
            | "while" expr block
            | "match" expr "{" { arm } "}"
            | "try" block "catch" NAME block
            | "with" NAME "=" NAME "{" [ entry { "," entry } [ "," ] ] "}" block   (* test の中だけ *)
            | "expect" "(" expr ")" ";"                                            (* test の中だけ *)
            | expr [ ( "=" | "+=" | "-=" | "*=" | "/=" | "%=" ) expr ] ";" ;
if_stmt     = "if" expr block [ "else" ( if_stmt | block ) ] ;
arm         = ( "ok" | "err" | "some" ) "(" NAME ")" block | "none" block ;
entry       = "{" expr "," expr "}" ;

expr        = or ;
or          = and { "||" and } ;
and         = cmp { "&&" cmp } ;
cmp         = add { ( "==" | "!=" | "<" | "<=" | ">" | ">=" ) add } ;
add         = mul { ( "+" | "-" ) mul } ;
mul         = unary { ( "*" | "/" | "%" ) unary } ;
unary       = ( "-" | "!" ) unary | postfix ;
postfix     = primary { "(" [ list ] ")" | "." NAME | "[" expr "]" | "?" } ;
primary     = INT | FLOAT | STRING | "true" | "false" | "none" | "self"
            | NAME [ "::" NAME ]
            | "(" expr ")" | "[" [ list ] "]" ;
list        = expr { "," expr } [ "," ] ;
```

文法上は書けても、検査で拒否されるものがあります（`self` は `where` の中だけ、`old()` は `post` の中だけ、など）。
