# C.C. 実装計画

- 対象: [CONCEPT.md](CONCEPT.md) ドラフト 0.5
- 方針: **動くものを最短で作り、1ステップごとに動作を確認する**

---

## 1. 状況

C++ の上位互換としての実装は、次の8ステップまで完了している。確認はすべて `cargo test` に含まれる（Ubuntu 24.04、g++ 13.3、clang 20.1）。

| 順 | 作業 | 状態 | 確認方法 |
|---|---|---|---|
| 1 | Clang の検証 | 完了 | 2章 |
| 2 | 上位互換の土台 | 完了 | `compat`（`tests/compat/plain.cpp`、`examples/hand/`） |
| 3 | `type` と `schema` | 完了 | `run`、`errors`、`check_reports_cpp_errors_against_the_source`（`examples/user.c.c`、`tests/run/refine_range.c.c`、`tests/run/schema_values.c.c`） |
| 4 | `pre` / `post` | 完了 | `run`（`examples/withdraw.c.c`、`tests/run/enforce.c.c`、`tests/run/forms.c.c`）、`errors`（`tests/errors/contract_forms.c.c`） |
| 5 | 厳格モードとテスト | 完了 | `tests/run/strict_index.c.c`、`check_accepts_what_cpp_accepts`、`test_blocks` |
| 6 | 例と文書 | 完了 | `examples/`、`README.md`、`docs/language.md` |
| 7 | 効果の検査 | 完了 | `effects`（`tests/effects/`）、`a_print_added_to_confcheck_is_reported`、`effects_without_clang_warn` |
| 8 | 拡張付きヘッダー | 完了 | `extended_headers`（`tests/headers/`） |

---

## 2. Clang の検証の結果

| 確かめたこと | 結果 |
|---|---|
| C++23 のヘッダーを読めるか | clang 18 と libstdc++ 13 は `std::expected` を読めない。**clang 19 と 20 は読める**。必要なのは clang 19 以降 |
| 注釈を AST で読めるか | `[[cc::effects(...)]]` は未知の属性として捨てられる。`[[clang::annotate("cc", "effects:io")]]` は残る。JSON に出るのは第2引数だけなので、そこに内容を入れる |
| 見えない呼び出しを検出できるか | できる（下の表） |
| 現実的な速さか | `nlohmann/json.hpp` を含むと JSON は約 1GB、ダンプに約 5 秒、読み取りを含めて 1 ファイル 10 秒以内。Rust でストリームとして読めば足りる。`[[cc::effects]]` のないファイルでは Clang を起動しない |
| g++ と共存できるか | g++ は `cc::` の属性に警告を出す。`ccc` がマクロに置き換え、g++ では空になるようにした（`[[ ]]` は有効な C++） |

**判断:** LibTooling は不要。`-ast-dump=json` で進めた。

呼び先の特定：

| 種類 | AST での現れ方 |
|---|---|
| 関数、ADL、マクロ、演算子、テンプレートの実体 | `DeclRefExpr` の `referencedDecl`（実体の ID を指す） |
| メンバ関数 | `MemberExpr` の `referencedMemberDecl` |
| コンストラクタ | `CXXConstructExpr`。**ID がない**ので、型の名前と `ctorType` で引く |
| 変数のデストラクタ | **ノードがない**ので、`VarDecl` の型から引く |
| 一時オブジェクトのデストラクタ | `CXXBindTemporaryExpr` の `dtor` |
| ラムダ | `LambdaExpr` の中の本体。呼び出し元の一部として調べる |
| 関数ポインタ | `CallExpr` の先頭の子が関数を直接指していない |
| `new`、`delete`、`throw` | `CXXNewExpr`、`CXXDeleteExpr`、`CXXThrowExpr` |

注意点：JSON の位置情報は変化したときだけ出力されるので、読み飛ばす部分も `file` と `line` を追う必要がある。属性は関数の本体より後に出力されるので、本体の呼び出しを集めてから注釈を見て判断する。

---

## 3. 実装中に決めたこと

| 項目 | 決定 | 理由 |
|---|---|---|
| 拡張の判定 | 語だけでなく形で判定する。`type` は `where` か `matches` を伴うとき、`schema` はフィールドか `invariant` で始まるとき、`pre` / `post` は宣言子の直後で本体か `;` が続くとき。同名のマクロがあれば拡張としない | 有効な C++ を壊さないため |
| `#if` と波括弧 | `#else` で `#if` のときのスコープに戻す | 分岐ごとに波括弧が揃っていないコードがあるため |
| 行番号の保持 | 拡張の外は1文字も変えない。生成コードの前後に `#line` を置き、後続の字句は空白で桁も揃える | エラーと違反の報告が `.c.c` の位置を指すため |
| `<cc/cc.hpp>` の取り込み | 拡張を使うファイルだけ。効果の注釈だけのファイルは軽い `<cc/effects.hpp>` | 普通の C++ に影響を与えないため |
| `test` の実行 | 利用者の `main` に触らない。ファイルの最後の静的初期化の中でテストを実行して終了する。`main` がなければ空の `main` を足す | C++ の部分を変えないため |
| 宣言と定義の分離 | `pre` / `post` は定義にだけ書ける。先に同じ引数の宣言があるとオーバーロードが曖昧になり、C++ のエラーになる | 隠し引数でシグネチャが変わるので、黙って検査が抜けないようにする |
| `io` と `alloc` | `io` は `alloc` を含む。`std::bad_alloc` は `throw` に数えない | CONCEPT 7.3 の例が `io, throw` だけで通るように。すべての確保に `throw` を要求しないように |
| 生成コードの効果 | `CC_TRUSTED` で宣言し、本体は検査しない。`check` と `try_from` は確保しない（`RefineError` はリテラルだけを持つ） | 条件付きの型を `effects()` の関数で使えるように |
| 検査用の出力 | `ccc check` は Clang 用に別の変換をする。条件は残し、違反の報告は出さない | 報告のための確保を、利用者の関数の効果に数えないため |
| エラーの要約 | 既定では `error:` の行だけを出し、候補の一覧（`note:`）は `CC_VERBOSE=1` で見せる | C++ を直接書くので必要な場面は増えるが、既定では短いほうが読める |
| `.c.h` の変換 | 実行のたびに作り直す。出力先に `.h` として置く（`ccc build`）。`ccc` が作ったのではないファイルは上書きしない | 古い変換結果が残らないようにするため |
| `ccc audit` | 作らない | 今は `grep` で足りる。実際に要る場面が来てから考える |

---

## 4. 既知の制限

- `pre` / `post` は名前空間のスコープの普通の関数だけ。`template`、メンバ関数、可変長引数、`requires` の付いた関数、演算子はエラーになる
- `pre` / `post` のある関数は隠し引数を持つので、関数ポインタの型が変わる
- `invariant` の中の式は C++ の一部（算術、比較、論理、呼び出し、`.`、`[]`、`::`）だけ。`->`、三項演算子、キャストは関数に切り出す
- `invariant`、`matches`、`transform` から呼ぶ関数は、スキーマより前に宣言する（C++ の規則どおり）
- スキーマで使う `enum` は値を指定できず、同じファイルで宣言されたものに限る。ほかの `enum` は `cc_enum_names` を自分で書く
- `with` にあたる差し替えは `cc::read_file` だけ。`cc::print` は本物のまま出力される
- `ccc test` は `fork` を使うので POSIX 専用。1ファイルずつ実行し、テストの絞り込みはできない
- `ccc` は1回に1つの `.c.c` を扱う。複数のファイルに分かれたプログラムは、`ccc build` の出力を自分のビルドに組み込む
- `.c.h` の変換先の名前はディレクトリを含まないので、同じ名前の `.c.h` を一緒に使えない
- 効果の一覧は標準ライブラリの主な部分だけ。載っていない関数は `unsafe` になる
- 効果の検査は `catch (...)` のある `try` だけを `throw` の例外にする。特定の型の `catch` では `throw` が必要なまま
- 効果の検査は暗黙に定義された特殊メンバ関数の中を見ない。自明でなければ `unsafe` になる
- 動作を確認しているのは Ubuntu 24.04、g++ 13、clang 19 / 20 だけ

---

## 5. 残っている作業

| 候補 | 内容 |
|---|---|
| `pre` / `post` の対象を広げる | メンバ関数、`template` |
| C++26 Contracts への対応 | コンパイラが揃ったら、`pre` / `post` の説明の文字列との共存を見直す |
| Z3 による証明 | `where` と `pre` を SMT ソルバーに渡し、証明できた実行時チェックを消す。式の型は Clang の AST で分かる |
| テスト自動生成、JSON Schema の出力 | スキーマから生成する |
| 所有（`once`、`fixed`）と手順（`protocol`） | CONCEPT 9 章 |

---

## 6. テストの方針

`cargo test` 一つですべて走る。`UPDATE=1 cargo test` で期待値を更新する。

| テスト | 内容 |
|---|---|
| `compat` | 普通の C++ を `.c.c` として変換しても1文字も変わらず、実行結果も同じ |
| `snapshot` | `examples/*.c.c` → `tests/snapshot/*.cpp` の出力 |
| `run`、`confcheck_cli` | 生成した C++ を g++ で `-Wall` の警告なしにビルドし、出力を比較 |
| `errors` | 不正な `.c.c` に対する `ccc` のエラー |
| `check_reports_cpp_errors_against_the_source` | C++ コンパイラのエラーが `.c.c` の行で分かりやすく出る |
| `test_blocks` | `test` ブロックの実行結果 |
| `effects` | Clang による効果の検査。CI では Clang がないと失敗する |
| `extended_headers` | `.c.h` の変換と利用 |
