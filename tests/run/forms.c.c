// The forms a function with a contract can take.
#include <cc/cc.hpp>

#include <iostream>
#include <memory>
#include <string>
#include <vector>

// Calls itself: the recursive call goes through the checks.
int factorial(int n)
  pre(n >= 0, "負でない")
  post(r: r >= 1)
{
  return n <= 1 ? 1 : n * factorial(n - 1);
}

// Default values stay with the function.
int scaled(int x, int by = 3) pre(by != 0) { return x * by; }

// No return value.
void bump(int& x) post(x == old(x) + 1) { ++x; }

// A trailing return type and noexcept.
auto half(int x) noexcept -> int pre(x % 2 == 0, "偶数") { return x / 2; }

// A parameter changed by the body: post sees the value the caller passed.
int consume(int x) post(r: r == x * 2) {
  int original = x;
  x = 0;
  return original * 2 + x;
}

// Passed by value and moved along, without a copy.
std::size_t take(std::unique_ptr<int> p, std::string s) pre(p != nullptr) { return s.size() + static_cast<std::size_t>(*p); }

// Returning a reference.
int& first(std::vector<int>& v) pre(!v.empty()) { return v[0]; }

namespace bank {
struct Account {
  int balance;
};
inline int fee(const Account& a) pre(a.balance >= 0) { return a.balance / 10; }
static int doubled(int x) post(r: r == 2 * x) { return 2 * x; }
}  // namespace bank

[[nodiscard]] int kept(int x) pre(x > 0) { return x; }

// A function pointer parameter.
int apply(int (*f)(int), int x) pre(f != nullptr) { return f(x); }
int twice(int x) { return x * 2; }

int main() {
  std::cout << factorial(5) << "\n";
  std::cout << scaled(2) << " " << scaled(2, 5) << "\n";
  int n = 1;
  bump(n);
  std::cout << n << "\n";
  std::cout << half(4) << "\n";
  std::cout << consume(21) << "\n";
  std::cout << take(std::make_unique<int>(2), "abc") << "\n";
  std::vector<int> v{1, 2};
  first(v) = 9;
  std::cout << v[0] << "\n";
  std::cout << bank::fee({100}) << " " << bank::doubled(4) << "\n";
  std::cout << kept(1) << " " << apply(twice, 4) << "\n";
  std::cout << factorial(-1) << "\n";
}
