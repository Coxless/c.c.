// Under `ccc run` signed overflow stops the program, with or without strict mode.
#include <cc/cc.hpp>

int main() {
  std::int64_t x = 9223372036854775807;
  cc::print(x);
  x += 1;
  cc::print("届かない", x);
  return 0;
}
