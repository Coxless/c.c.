// A failure in one test must not stop the others.
#include <cc/cc.hpp>

#include <string>

std::int64_t half(std::int64_t x)
  pre(x % 2 == 0, "偶数")
{
  return x / 2;
}

schema Id { id: i64; }

std::int64_t parse_id(const std::string& text) {
  return cc::must(Id::parse(cc::parse_json(text))).id;
}

// A program with a main of its own still runs its tests instead.
int main() {
  cc::print("main は実行されない");
  return 1;
}

test "通る" {
  expect(half(4) == 2);
}

test "expect が外れても続きを実行する" {
  int n = 3;
  expect(n == 4);
  expect(n == 5);
  cc::print("最後まで来た");
}

test "pre 違反で止まっても次へ進む" {
  cc::print(half(3));
}

test "送出された失敗" {
  cc::print(parse_id("{}"));
}

test "std::exception ではない値" {
  throw 1;
}

test "ハンドラはスコープの外に漏れない" {
  {
    cc::MemoryFs fs{{"a", "1"}};
    cc::WithIo io{fs};
    expect(cc::read_file("a") == "1");
  }
  expect(cc::read_file("a") == "1");
}
