// Under `ccc run` signed overflow stops the program instead of wrapping.
fn main() -> i32 effects(io) {
  let x: i64 = 9223372036854775807;
  print(x);
  x += 1;
  print("届かない", x);
  return 0;
}
