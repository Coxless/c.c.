// parse and make go through the same checks, and an invariant cannot be broken by assignment.
#include <cc/cc.hpp>

enum class Level { low, high };

schema Limits {
  name: string where len(self) >= 1 transform trim;
  level: Level = Level::low;
  max: i64 where self > 0 = 10;

  invariant(level != high || max >= 100, "high には 100 以上が必要");
}

open schema Loose {
  a: i64;
}

schema Plain {
  a: i64 where self > 0;
  b: string;
}

namespace inside {
type Id = u32 where self > 0;
schema Item {
  id: Id;
}
}  // namespace inside

int main() {
  cc::print(Limits::parse(cc::parse_json(R"({"name": " x "})")));
  cc::print(Limits::parse(cc::parse_json(R"({"name": "x", "level": "high", "max": 5})")));
  cc::print(Limits::parse(cc::parse_json(R"({"name": "x", "level": "other"})")));
  cc::print(Limits::make({.name = " y ", .level = Level::high, .max = 100}));
  cc::print(Limits::make({.name = "", .level = Level::high, .max = 5}));
  cc::print(Limits::make({}));

  auto a = cc::must(Limits::make({.name = "a"}));
  auto b = cc::must(Limits::make({.name = "b", .max = 3}));
  a = b;
  cc::print(a.name, a.max);
  std::vector<Limits> all;
  all.push_back(a);
  all.push_back(std::move(b));
  cc::print(all.size());

  cc::print(Loose::parse(cc::parse_json(R"({"a": 1, "other": 2})")));

  // Without an invariant, a schema is a plain struct whose fields check themselves.
  Plain p{5, "text"};
  p.a = 6;
  p.b = "changed";
  cc::print(p.a, p.b);

  cc::print(inside::Item::parse(cc::parse_json(R"({"id": 0})")));
  inside::Id id = 7;
  cc::print(id);
}
