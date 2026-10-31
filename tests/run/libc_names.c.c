// A function may share its name with the C library, as in C++: this is an overload.
#include <cc/cc.hpp>

#include <cstdlib>

std::int64_t abs(std::int64_t a, std::int64_t b)
  pre(b != 0)
{
  return (a < 0 ? -a : a) / b;
}

int main() {
  cc::print(abs(-6, 2));
  cc::print(abs(-1));
}
