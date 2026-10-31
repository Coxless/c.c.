// Effects: what a function may do is in its signature, and `ccc check` holds it to that.
#include <cc/cc.hpp>

#include <string>

[[cc::effects()]]
int add(int a, int b) {
  return a + b;
}

schema Config {
  port: i64 where 1 <= self <= 65535;
}

[[cc::effects(io, throw)]]
Config load_config() {
  return cc::must(Config::parse(cc::parse_json(cc::read_file("app.json"))));
}

// Handlers replace what the function reads, so the same code runs in tests without a mock library.
test "設定を読める" {
  cc::MemoryFs fs{{"app.json", R"({"port": 8080})"}};
  cc::WithIo io{fs};
  expect(load_config().port == 8080);
  expect(add(1, 2) == 3);
}

[[cc::effects(io)]]
int main() {
  cc::print(add(1, 2));
  return 0;
}
