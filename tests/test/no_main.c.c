// Tests in a file without a main.
#include <vector>

int sum(const std::vector<int>& v) {
  int total = 0;
  for (int x : v) total += x;
  return total;
}

test "合計" {
  std::vector<int> v{1, 2};
  if (v.size() == 2) expect(sum(v) == 3); else expect(false);
}
