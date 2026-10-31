// A C++ file with contracts added. Everything except `type`, `schema`, `pre` and `post` is plain C++.
#include <cc/cc.hpp>

#include <iostream>
#include <string>

type Port = u16 where 1 <= self <= 65535;

schema Server {
  name:    string where 1 <= len(self) <= 50 transform trim;
  port:    Port;
  workers: i64    where 1 <= self <= 256 = 4;
}

std::int64_t per_worker(std::int64_t requests, std::int64_t workers)
  pre(workers > 0, "ワーカーが1つ以上ある")
  post(r: r * workers <= requests, "割り当ての合計が総数を超えない")
{
  return requests / workers;
}

void load(const std::string& text) {
  if (auto s = Server::parse(cc::parse_json(text))) {
    std::cout << s->name << " port " << s->port << " workers " << s->workers << "\n";
  } else {
    std::cout << s.error().to_string() << "\n";
  }
}

int main() {
  Port admin = 8443;
  std::cout << "admin port " << admin << "\n";
  load(R"({"name": " edge ", "port": 8080})");
  load(R"({"name": "", "port": 70000, "workers": 0, "wokers": 8})");
  std::cout << per_worker(100, 3) << std::endl;
  std::cout << per_worker(100, 0) << std::endl;
}
