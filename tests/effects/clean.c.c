// What the check must accept.
#include <cc/cc.hpp>

#include <optional>
#include <string>
#include <vector>

type Port = u16 where 1 <= self <= 65535;

schema Config {
  port: Port;
  name: string where len(self) >= 1;

  invariant(port != 1, "1 番は使えない");
}

struct Point {
  int x = 0;
  int y = 0;
};

[[cc::effects()]] int add(int a, int b) { return a + b; }

// Plain data, comparisons and generated types do nothing.
[[cc::effects()]] int pure_things(Point p, const Config& c, const std::vector<int>& v) {
  Point q = p;
  Port literal = 8080;
  auto parsed = Port::try_from(q.x);
  int total = add(q.x, q.y) + literal + static_cast<int>(v.size());
  for (int x : v) total += x;
  if (parsed && *parsed == literal) total += 1;
  if (c.port == 80 && c.name == std::string_view("n")) total += 1;
  auto f = [&](int a) { return add(a, total); };
  return f(1);
}

// `io` covers `alloc`, since input and output cannot do without.
[[cc::effects(io)]] void prints(const std::vector<std::string>& lines) {
  std::string joined = "x";
  for (const auto& l : lines) joined += l;
  cc::print(joined);
}

[[cc::effects(alloc)]] std::vector<int> build() {
  std::vector<int> v;
  v.push_back(1);
  return v;
}

[[cc::effects(throw)]] int thrower(int x) {
  if (x < 0) throw x;
  return x;
}

// `catch (...)` keeps it from getting out.
[[cc::effects()]] int caught(int x) {
  try {
    return thrower(x);
  } catch (...) {
    return 0;
  }
}

[[cc::effects(io, throw)]] Config load(const std::string& path) {
  return cc::must(Config::parse(cc::parse_json(cc::read_file(path))));
}

// Functions without an annotation stay ordinary C++ and can do anything.
void anything() {
  int* p = new int(1);
  cc::print(*p);
  delete p;
}

[[cc::effects(unsafe)]] void trusted_caller() { anything(); }

[[cc::effects()]] int contract_ok(int x)
  pre(x > 0)
  post(r: r == x)
{
  return x;
}

// A template is judged for each instance, and this instance is fine.
template <class T>
[[cc::effects()]] T same(T x) { return x; }
[[cc::effects()]] int use_same() { return same(1); }

// A member function defined outside its class, and other attributes next to the effects.
struct Counter {
  int n = 0;
  [[nodiscard, cc::effects()]] int next();
};
int Counter::next() { return ++n; }
[[cc::effects()]] int use_counter() {
  Counter c;
  return c.next();
}

int main() {
  cc::print(contract_ok(1), caught(-1), same(2), use_same());
}
