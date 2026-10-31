// Strict mode turns on the bounds checks of the standard library.
#pragma cc strict
#include <cc/cc.hpp>

#include <vector>

int main() {
  std::vector<int> v{10, 20, 30};
  cc::print(v[2]);
  std::size_t i = 3;
  cc::print(v[i]);
  cc::print("届かない");
  return 0;
}
