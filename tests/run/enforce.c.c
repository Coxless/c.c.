// The default mode is enforce: the first violation stops the program.
#include <cc/cc.hpp>

#include <iostream>

std::int64_t half(std::int64_t x)
  pre(x % 2 == 0, "偶数")
{
  return x / 2;
}

int main() {
  std::cout << half(4) << std::endl;
  std::cout << half(3) << std::endl;
  std::cout << "ここには来ない" << std::endl;
}
