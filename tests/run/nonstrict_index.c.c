// Without strict mode C++ means what it means: this builds with no bounds check turned on.
#include <vector>
#include <iostream>

#ifdef _GLIBCXX_ASSERTIONS
#error strict mode was not asked for
#endif

int main() {
  std::vector<int> v{10, 20, 30};
  long small = 2;
  int narrowed = small;  // an implicit narrowing conversion is fine here
  std::cout << v[static_cast<std::size_t>(narrowed)] << std::endl;
}
