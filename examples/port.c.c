// Refined types: a literal is checked at compile time, any other value through try_from.
#include <cc/cc.hpp>

#include <iostream>

type Port = u16 where 1 <= self <= 65535;

int main() {
  Port http = 8080;
  std::cout << http << "\n";
  std::cout << (http == 8080 ? "same" : "different") << "\n";

  for (int n : {443, 0, 70000}) {
    if (auto p = Port::try_from(n)) {
      std::cout << "accepted " << *p << "\n";
    } else {
      cc::print(p.error());
    }
  }
}
