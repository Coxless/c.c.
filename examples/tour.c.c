// README の例。値の契約、スキーマ、関数の契約を一通り使う。
//
//   ccc run examples/tour.c.c

type Port = u16 where 1 <= self <= 65535;

schema Server {
  name:    string where 1 <= len(self) <= 50 transform trim;
  port:    Port;
  workers: i64    where 1 <= self <= 256 = 4;
}

contract fn per_worker(requests: i64, workers: i64) -> i64
  effects()
  pre(workers > 0, "ワーカーが1つ以上ある")
  post(r: r * workers <= requests, "割り当ての合計が総数を超えない")
{
  return requests / workers;
}

fn load(text: string) effects(io) {
  match Server::parse(parse_json(text)) {
    ok(s)  { print(s.name, "port", s.port, "workers", s.workers); }
    err(e) { print(e); }
  }
}

fn main() -> i32 effects(io) {
  load(R"({"name": " edge ", "port": 8080})");
  load(R"({"name": "", "port": 70000, "workers": 0, "wokers": 8})");
  print(per_worker(100, 3));
  print(per_worker(100, 0));
  return 0;
}
