// Valid C++ stays valid: nothing in this file may be touched. It uses every word that
// C.C. gives a meaning to, as ordinary names, and the things a careless scanner trips on.
#include <algorithm>
#include <iostream>
#include <map>
#include <memory>
#include <string>
#include <vector>

#define OPEN_BLOCK {
#define CLOSE_BLOCK }
#define LOG(x) std::cout << "log: " << (x) << "\n"
#define STRINGIFY(x) #x

// Names that C.C. treats as keywords only in the right place.
using type = int;
type where = 1;
type matches = 2;
struct schema {
  int value;
};
schema s1{1};
schema s2{};
namespace test {
int value = 3;
}
int pre(int x) { return x + 1; }
int post(int x) { return x + 2; }
int expect(bool b) { return b ? 10 : 20; }

struct Holder {
  int pre = 4;
  int post(int x) const { return x * pre; }
};

type x1 = 5;

// Number and string forms.
const long big = 1'000'000;
const unsigned hex = 0xFF'FF;
const double sci = 1.5e+3;
const char quote = '\'';
const char brace = '{';
const char* open_brace = "{";
const char* raw = R"delim(unbalanced { and " and )" here)delim";
const char8_t* prefixed = u8R"(also { unbalanced)";
/* an unbalanced comment { */
// and another {

#if 0
int broken( {
#else
int chosen() {
#endif
  return 7;
}

#ifdef NOT_DEFINED
namespace unused {
#else
namespace used {
#endif
int in_namespace() { return 8; }
}

// Classes, inheritance and virtual functions.
class Shape {
 public:
  virtual ~Shape() = default;
  virtual int area() const = 0;
};
class Square : public Shape {
  int side_;

 public:
  explicit Square(int side) : side_(side) {}
  int area() const override { return side_ * side_; }
};

// Templates.
template <class T, int N = 2>
struct Scaled {
  T value;
  T get() const { return value * N; }
};
template <class T>
T largest(const std::vector<T>& v) {
  return *std::max_element(v.begin(), v.end());
}
template <class... A>
int count(A...) {
  return sizeof...(A);
}

enum class Color { red, green = 5 };
enum Old { a_value, b_value };

int with_block() OPEN_BLOCK
  return 9;
CLOSE_BLOCK

int main() {
  std::cout << where + matches + s1.value + s2.value + test::value << "\n";
  std::cout << pre(1) << " " << post(1) << " " << expect(true) << "\n";
  Holder h;
  std::cout << h.post(2) << " " << x1 << "\n";
  std::cout << big << " " << hex << " " << sci << " " << quote << brace << open_brace << "\n";
  std::cout << raw << " " << reinterpret_cast<const char*>(prefixed) << "\n";
  std::cout << chosen() << " " << used::in_namespace() << " " << with_block() << "\n";
  std::unique_ptr<Shape> shape = std::make_unique<Square>(3);
  std::cout << shape->area() << "\n";
  std::cout << Scaled<int>{4}.get() << " " << largest<int>({1, 5, 2}) << " " << count(1, 2, 3) << "\n";
  auto add = [offset = 1](int a) { return a + offset; };
  std::map<std::string, int> m{{"k", add(1)}};
  std::cout << m["k"] << " " << static_cast<int>(Color::green) << " " << b_value << "\n";
  LOG(STRINGIFY(type schema pre post test));
  return 0;
}
