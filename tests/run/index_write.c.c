// A negative index, and on the left of an assignment. In observe mode it stops all the same.
fn main() -> i32 effects(io, unsafe) {
  cpp { cc::contract_mode = cc::Mode::observe; }
  let v = [1];
  v[0] = 2;
  print(v[0]);
  let i = -1;
  v[i] = 3;
  print("届かない");
  return 0;
}
