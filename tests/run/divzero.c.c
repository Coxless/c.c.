#include <iostream>

int main(int argc, char**) {
  int zero = argc - 1;
  std::cout << "before" << std::endl;
  std::cout << 1 / zero << std::endl;
  std::cout << "after" << std::endl;
}
