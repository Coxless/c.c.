// Functions named like C library functions are still this file's functions,
// and their contracts are still checked.
fn div(a: i64, b: i64) -> i64 effects() {
  return a / b;
}

fn rand() -> i64 effects() {
  return 4;
}

contract fn abs(a: i64) -> i64
  effects()
  pre(a != 0, "0 以外")
{
  return 42;
}

schema S {
  n: i64;
  invariant(abs(n) == 42, "自作の abs を呼ぶ");
}

fn main() -> i32 effects(io) {
  let x: i32 = 6;
  let y: i32 = 3;
  print(div(x, y), div(6, 3), rand());
  match S::parse(parse_json(R"({"n": 1})")) {
    ok(s) { print("invariant ok"); }
    err(e) { print(e); }
  }
  print(abs(-1));
  print(abs(0));
  print("届かない");
  return 0;
}
