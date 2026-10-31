// Under `ccc run` division by zero stops the program with a message.
fn div(a: i64, b: i64) -> i64 effects() {
  return a / b;
}

fn main() -> i32 effects(io) {
  print(div(6, 3));
  print(div(1, 0));
  print("届かない");
  return 0;
}
