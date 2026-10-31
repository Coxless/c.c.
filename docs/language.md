# C.C. 言語リファレンス

実装済みの言語機能をまとめます。考え方と将来の構想は [CONCEPT.md](../CONCEPT.md)、制限の一覧は [PLAN.md](../PLAN.md) にあります。

## 宣言

```cpp
type Port  = u16 where 1 <= self <= 65535;     // 条件付きの型（ブランド型）
type Email = string matches email_format;       // 述語関数による条件
enum Role { admin, member }                     // 列挙（enum { a, b } はフィールドにも直接書ける）

schema User {
  id:    u64         where self > 0;
  name:  string      where 1 <= len(self) <= 50 transform trim;
  email: Email;
  age:   int?        where 0 <= self <= 150;    // T? は省略可能
  tags:  vec<string> where len(self) <= 3 = [];  // 既定値
  invariant(age == none || age >= 13, "13歳以上");
}
open schema Event { id: u64; }                  // 未知のフィールドを許す

contract fn withdraw(a: Account&, amt: i64) -> i64
  effects(log)
  pre(amt > 0, "金額は正")
  post(r: r == amt)
  post(a.balance == old(a.balance) - amt)
{ ... }

fn greet(u: User) effects(io) { ... }
fn main(args: vec<string>) -> i32 effects(io) { ... } // 引数は省略できる。プログラム名は含まない
extern fn open_account(b: i64) -> Account effects();  // C++ で実装された関数の宣言
cpp { /* 生の C++（逃げ道） */ }
test "名前" { expect(1 + 1 == 2); }
```

- 型: `i32` `i64`（`int`）`u8` `u16` `u32` `u64` `f64` `bool` `string`（`str`）`vec<T>` `T?` `Json` `Raw<T>`
- 文: `let`（`let x: T = ...` も可）、代入（`=` `+=` など）、`return`、`if` / `else`、`for x in xs`、`for i in a..b`、`while`、`match`、`try` / `catch`、`cpp { ... }`
- 式: 算術、比較（`1 <= x <= 9` のような連鎖も可）、論理、関数呼び出し、`T::parse` / `T::try_from`、フィールド・添字・メソッド呼び出し、`none`、`[a, b]`、`R"(...)"`
- 組み込み関数: `len` `trim` `email_format` `parse_json`（純粋）、`print` `eprint`（io）、`log`（log）、`read_file`（io, throw）

## 値の契約

- `Port p = 8080;` に相当する `let p: Port = 8080;` は `Port::of<8080>()` になり、条件はコンパイル時に検査されます
- 静的に分からない値（`n + 1` など）から暗黙には変換できません。`Port::try_from(...)` を使います
- `try_from` は、受け取った整数が元の型の範囲に収まるかを先に調べます（`Port::try_from(73616)` はエラーです）。小数は渡せません
- 値が変わりうる暗黙の変換（`i64` → `u16`、`i64` → `u64`、`f64` → `i64` など）と、符号付きと符号なしの比較はコンパイルエラーです
- `let x = 0;` や `[1, 2, 3]` の整数リテラルは `i64` になります
- 外部入力は `Raw<Json>` で受け取り、`User::parse(...)` を通したものだけが `User` になります。`Raw<Json>` を `User` として渡すとコンパイルエラーです
- スキーマにない JSON フィールドはエラーです。`open schema` にすると無視します
- `where` に書けるのはリテラル・`self`・`len()`・算術・比較・論理だけです。それ以外の条件は `matches` か `invariant` に書きます。`invariant` からは純粋な関数を呼べます

## 失敗の扱い

```cpp
let user = User::parse(body)?;          // 失敗を送出する（throw が必要）

match User::parse(body) {               // ok / err の両方が必要
  ok(user) { greet(user); }
  err(e)   { eprint(e); }
}

match user.age {                        // T? は some / none
  some(age) { print(age); }
  none      { print("年齢なし"); }
}

try {                                   // 送出された失敗を文字列で受け取る。中の throw は外に出ない
  let text = read_file(path);
} catch message {
  eprint(message);
}
```

## 関数の契約

- `pre` の違反は呼び出し側、`post` の違反は実装側の責任として、`.c.c` の行番号付きで報告します
- 既定では違反で停止します。`-DCC_CONTRACTS_OBSERVE`（または `cc::contract_mode = cc::Mode::observe;`）で報告だけして続行します
- `v[i]` の範囲外は、呼び出し側の責任の契約違反として報告します。続行する方法がないので、observe でも必ず停止します
- 値を返すと宣言した関数が `return` せずに終わる経路は、コンパイルエラーです

## 効果

- 関数が呼ぶものの効果は、その関数が `effects(...)` で宣言した効果に含まれていなければなりません
- `effects(...)` を省略した関数は検査されず、何でもできます（既存コードから段階的に導入するため）
- 効果が分からないもの（`effects` を省略した関数、`extern fn` で宣言していない C++ 関数、関数内の `cpp { }`、C++ のメソッド）を呼ぶには `unsafe` が必要です。生成された `to_json` と `get` は純粋です
- `?` は失敗時に例外を送出するので `throw` が必要です。`throw` を宣言していない関数には `noexcept` が付きます
- `where` / `pre` / `post` / `invariant` の条件と既定値は純粋でなければなりません

## テスト

```cpp
test "設定を読める" {
  with io = MemoryFs{ {"a.json", R"({"port": 8080})"} } {   // read_file を差し替える（test の中だけ）
    let c = load_config("a.json");
    expect(c.port == 8080);                                  // 外れても続きを実行し、失敗として数える
  }
}
```

`ccc test` で実行します。テストごとに別プロセスで動くので、契約違反で止まっても残りのテストは実行されます。テストが1件でも失敗すると終了コードは 1 です。`ccc run` では `test` は無視されます。
