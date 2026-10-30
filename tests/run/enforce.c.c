// Default mode is enforce: the first violation aborts the program.
contract fn half(i64 x) -> i64 effects()
  pre(x % 2 == 0, "偶数")
{
  return x / 2;
}

fn main() effects(io) {
  print(half(4));
  print(half(3));
  print("ここには来ない");
}
