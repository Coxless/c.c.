# 同梱しているサードパーティのコード

## nlohmann/json 3.11.3

- 取得元: https://github.com/nlohmann/json/releases/tag/v3.11.3 の `json.hpp`（変更なし）
- `json.hpp` の SHA-256: `9bea4c8066ef4a1c206b2be5a36302f8926f7fdc6087af5d20b417d0cf103ea6`
- ライセンス: MIT（[nlohmann/LICENSE.MIT](nlohmann/LICENSE.MIT)）。Copyright (c) 2013-2023 Niels Lohmann

上流の README によると、`json.hpp` には次のコードも含まれています。

| コード | 著作権者 | ライセンス |
|---|---|---|
| UTF-8 デコーダ | Björn Hoehrmann | MIT |
| Grisu2 | Florian Loitsch | MIT |
| Hedley | Evan Nemerson | CC0-1.0 |
| Google Abseil の一部 | The Abseil Authors | Apache-2.0（[nlohmann/LICENSES/Apache-2.0.txt](nlohmann/LICENSES/Apache-2.0.txt)） |

## 生成したプログラムを配布するとき

C.C. が生成するコードは `json.hpp` を取り込みます。ビルドしたプログラムを配布するときは、上記のライセンス表示を同梱してください。
