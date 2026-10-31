// 設定ファイルを読んで検証する CLI。
//
//   ccc run  examples/confcheck.c.c -- examples/confcheck/good.json
//   ccc test examples/confcheck.c.c
//
// 終了コード: 0 = 正しい設定、1 = 不正な設定、2 = 使い方の誤り、3 = ファイルを読めない

type Port = u16 where 1 <= self <= 65535;

schema Upstream {
  host:   string where 1 <= len(self) <= 253 transform trim;
  port:   Port;
  weight: i64    where 1 <= self <= 100 = 1;
}

schema Tls {
  cert: string where len(self) >= 1;
  key:  string where len(self) >= 1;
}

schema Server {
  name:      string        where 1 <= len(self) <= 50 transform trim;
  listen:    Port;
  workers:   i64           where 1 <= self <= 256 = 4;
  log_level: enum { debug, info, warn, error } = info;
  upstreams: vec<Upstream> where len(self) >= 1;
  tls:       Tls?;

  invariant(listen != 443 || tls != none, "443 番で待ち受けるなら tls が必要");
  invariant(!has_duplicate(upstreams), "upstreams の host と port の組が重複している");
}

fn has_duplicate(upstreams: vec<Upstream>) -> bool effects() {
  for i in 0..len(upstreams) {
    for j in i + 1..len(upstreams) {
      if upstreams[i].host == upstreams[j].host && upstreams[i].port == upstreams[j].port {
        return true;
      }
    }
  }
  return false;
}

fn total_weight(upstreams: vec<Upstream>) -> i64 effects() {
  let sum = 0;
  for u in upstreams {
    sum += u.weight;
  }
  return sum;
}

contract fn share_percent(weight: i64, total: i64) -> i64
  effects()
  pre(1 <= weight <= total, "重みは合計以下")
  post(r: 0 <= r <= 100, "割合は 0〜100")
{
  return weight * 100 / total;
}

fn report(s: Server) effects(io) {
  print("OK:", s.name, "port", s.listen, "workers", s.workers, "log", s.log_level);
  let total = total_weight(s.upstreams);
  for u in s.upstreams {
    print(" ", u.host, u.port, share_percent(u.weight, total), "%");
  }
  print(s.to_json());
}

// 読み込みと検証の失敗をまとめて送出する。テストはこの関数を MemoryFs で動かす。
fn load_server(path: string) -> Server effects(io, throw) {
  return Server::parse(parse_json(read_file(path)))?;
}

fn main(args: vec<string>) -> i32 effects(io) {
  if len(args) != 1 {
    eprint("使い方: confcheck <config.json>");
    return 2;
  }
  let text = "";
  try {
    text = read_file(args[0]);
  } catch message {
    eprint("エラー:", message);
    return 3;
  }
  match Server::parse(parse_json(text)) {
    ok(server) {
      report(server);
      return 0;
    }
    err(e) {
      eprint(e);
      return 1;
    }
  }
}

test "既定値が入り、余白が取り除かれる" {
  with io = MemoryFs{ {"a.json", R"({"name": " edge ", "listen": 8080, "upstreams": [{"host": "x", "port": 1}]})"} } {
    let s = load_server("a.json");
    expect(s.name == "edge");
    expect(s.workers == 4);
    expect(s.log_level == info);
  }
}

test "443 番は tls がないと通らない" {
  match Server::parse(parse_json(R"({"name": "e", "listen": 443, "upstreams": [{"host": "x", "port": 1}]})")) {
    ok(s) { expect(false); }
    err(e) { expect(true); }
  }
}

test "重複した upstream は通らない" {
  let text = R"({"name": "e", "listen": 80, "upstreams": [{"host": "x", "port": 1}, {"host": " x ", "port": 1}]})";
  match Server::parse(parse_json(text)) {
    ok(s) { expect(false); }
    err(e) { expect(true); }
  }
}

test "未知のフィールドは通らない" {
  let text = R"({"name": "e", "listen": 80, "wokers": 2, "upstreams": [{"host": "x", "port": 1}]})";
  match Server::parse(parse_json(text)) {
    ok(s) { expect(false); }
    err(e) { expect(true); }
  }
}

test "ファイルがないと失敗を送出する" {
  with io = MemoryFs{} {
    try {
      load_server("none.json");
      expect(false);
    } catch message {
      expect(message == "ファイルを開けません: none.json");
    }
  }
}

test "重みの割合" {
  expect(share_percent(3, 4) == 75);
  expect(total_weight([]) == 0);
}
