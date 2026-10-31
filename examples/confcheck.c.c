// A command line tool that validates a configuration file.
//
//   ccc run   examples/confcheck.c.c -- examples/confcheck/good.json
//   ccc test  examples/confcheck.c.c
//   ccc check examples/confcheck.c.c
//
// Exit codes: 0 = valid, 1 = invalid, 2 = wrong usage, 3 = unreadable file.
#pragma cc strict

#include <cc/cc.hpp>

#include <cstdint>
#include <cstddef>
#include <exception>
#include <string>
#include <vector>

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

[[cc::effects()]]
bool has_duplicate(const std::vector<Upstream>& upstreams) {
  for (std::size_t i = 0; i < upstreams.size(); ++i) {
    for (std::size_t j = i + 1; j < upstreams.size(); ++j) {
      if (upstreams[i].host == upstreams[j].host && upstreams[i].port == upstreams[j].port) {
        return true;
      }
    }
  }
  return false;
}

schema Server {
  name:      string        where 1 <= len(self) <= 50 transform trim;
  listen:    Port;
  workers:   i64           where 1 <= self <= 256 = 4;
  log_level: enum { debug, info, warn, error } = info;
  upstreams: vec<Upstream> where len(self) >= 1;
  tls:       Tls?;

  invariant(listen != 443 || tls != none, "443 番で待ち受けるなら tls が必要");
  invariant(!has_duplicate(upstreams.get()), "upstreams の host と port の組が重複している");
}

[[cc::effects()]]
std::int64_t total_weight(const std::vector<Upstream>& upstreams) {
  std::int64_t sum = 0;
  for (const auto& u : upstreams) {
    sum += u.weight;
  }
  return sum;
}

[[cc::effects()]]
std::int64_t share_percent(std::int64_t weight, std::int64_t total)
  pre(1 <= weight && weight <= total, "重みは合計以下")
  post(r: 0 <= r && r <= 100, "割合は 0〜100")
{
  return weight * 100 / total;
}

[[cc::effects(io)]]
void report(const Server& s) {
  cc::print("OK:", s.name, "port", s.listen, "workers", s.workers, "log", s.log_level);
  const std::int64_t total = total_weight(s.upstreams.get());
  for (const auto& u : s.upstreams) {
    cc::print(" ", u.host, u.port, share_percent(u.weight, total), "%");
  }
  cc::print(s.to_json());
}

// Failures of reading and of validating come out the same way. The tests run this on a MemoryFs.
[[cc::effects(io, throw)]]
Server load_server(const std::string& path) {
  return cc::must(Server::parse(cc::parse_json(cc::read_file(path))));
}

int main(int argc, char** argv) {
  if (argc != 2) {
    cc::eprint("使い方: confcheck <config.json>");
    return 2;
  }
  std::string text;
  try {
    text = cc::read_file(argv[1]);
  } catch (const std::exception& e) {
    cc::eprint("エラー:", e.what());
    return 3;
  }
  auto server = Server::parse(cc::parse_json(text));
  if (!server) {
    cc::eprint(server.error());
    return 1;
  }
  report(*server);
  return 0;
}

test "既定値が入り、余白が取り除かれる" {
  cc::MemoryFs fs{{"a.json", R"({"name": " edge ", "listen": 8080, "upstreams": [{"host": "x", "port": 1}]})"}};
  cc::WithIo io{fs};
  Server s = load_server("a.json");
  expect(s.name == "edge");
  expect(s.workers == 4);
  expect(s.log_level == Server_log_level::info);
}

test "443 番は tls がないと通らない" {
  auto r = Server::parse(cc::parse_json(R"({"name": "e", "listen": 443, "upstreams": [{"host": "x", "port": 1}]})"));
  expect(!r);
}

test "重複した upstream は通らない" {
  auto r = Server::parse(cc::parse_json(R"({"name": "e", "listen": 80, "upstreams": [{"host": "x", "port": 1}, {"host": " x ", "port": 1}]})"));
  expect(!r);
}

test "未知のフィールドは通らない" {
  auto r = Server::parse(cc::parse_json(R"({"name": "e", "listen": 80, "wokers": 2, "upstreams": [{"host": "x", "port": 1}]})"));
  expect(!r);
}

test "ファイルがないと失敗を送出する" {
  cc::MemoryFs fs;
  cc::WithIo io{fs};
  try {
    load_server("none.json");
    expect(false);
  } catch (const cc::Failure& e) {
    expect(std::string(e.what()) == "ファイルを開けません: none.json");
  }
}

test "重みの割合" {
  expect(share_percent(3, 4) == 75);
  expect(total_weight({}) == 0);
}
