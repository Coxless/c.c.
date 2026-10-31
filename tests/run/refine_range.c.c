// try_from must judge the value as given, not after it has wrapped into the base type.
#include <cc/cc.hpp>

type Port = u16 where 1 <= self <= 65535;
type Small = i32 where self >= 0;
type Digit = u8 where self < 10;
type Wide = i64 where self >= 0;
type Ratio = f64 where 0 <= self <= 1;

schema Defaults {
  // -1 must not become a huge u64 that satisfies the condition.
  n: u64 where self > 0 = -1;
}

int main() {
  cc::print("port 8080", Port::try_from(8080).has_value());
  cc::print("port -1", Port::try_from(-1).has_value());
  cc::print("port 73616 (wraps to 8080)", Port::try_from(73616).has_value());
  cc::print("port 65536 (wraps to 0)", Port::try_from(65536).has_value());
  cc::print("small 4294967297 (wraps to 1)", Small::try_from(4294967297LL).has_value());
  cc::print("digit 261 (wraps to 5)", Digit::try_from(261).has_value());
  cc::print("digit 5", Digit::try_from(5).has_value());

  // From a refined value of a wider type.
  Wide wide = 73616;
  cc::print(Port::try_from(wide));
  // The range is reported as such when the condition itself would hold.
  cc::print(Small::try_from(4294967297LL));
  // Numbers, not characters.
  Digit d = 7;
  cc::print("digit", d);

  // Literals of every kind go through the same condition, at compile time.
  Ratio half = 0.5;
  cc::print(half, half == 0.5);
  Port p = 8080;
  cc::print(p == 8080, 8080 == p, p != 80, p < 9000);
  std::vector<Port> ports = {80, 443};
  cc::print(ports);

  cc::print(Defaults::parse(cc::parse_json("{}")));
}
