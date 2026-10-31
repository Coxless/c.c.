// try_from must judge the value as given, not after it has wrapped into the base type.
type Port = u16 where 1 <= self <= 65535;
type Small = i32 where self >= 0;
type Digit = u8 where self < 10;
type Wide = i64 where self >= 0;

schema Defaults {
  // -1 must not become a huge u64 that satisfies the condition.
  n: u64 where self > 0 = -1;
}

fn show(label: string, accepted: bool) effects(io) {
  print(label, accepted);
}

fn port(n: i64) -> bool effects() {
  match Port::try_from(n) {
    ok(p) { return true; }
    err(e) { return false; }
  }
}

fn small(n: i64) -> bool effects() {
  match Small::try_from(n) {
    ok(s) { return true; }
    err(e) { return false; }
  }
}

fn digit(n: i64) -> bool effects() {
  match Digit::try_from(n) {
    ok(d) { return true; }
    err(e) { return false; }
  }
}

fn main() -> i32 effects(io, throw) {
  show("port 8080", port(8080));
  show("port -1", port(-1));
  show("port 73616 (wraps to 8080)", port(73616));
  show("port 65536 (wraps to 0)", port(65536));
  show("small 4294967297 (wraps to 1)", small(4294967297));
  show("digit 261 (wraps to 5)", digit(261));
  show("digit 5", digit(5));

  // From a refined value of a wider type.
  let wide = Wide::try_from(73616)?;
  match Port::try_from(wide) {
    ok(p) { print("wide accepted", p); }
    err(e) { print("wide rejected:", e); }
  }
  // The range is reported as such when the condition itself would hold.
  match Small::try_from(4294967297) {
    ok(s) { print("accepted", s); }
    err(e) { print(e); }
  }
  // Numbers, not characters.
  let d = Digit::try_from(7)?;
  print("digit", d);
  let b: u8 = 65;
  print("u8", b);

  match Defaults::parse(parse_json("{}")) {
    ok(v) { print("default accepted", v.n); }
    err(e) { print(e); }
  }

  // Integer literals default to i64 rather than C++'s 32-bit int.
  let sum = 0;
  let big: i64 = 3000000000;
  sum += big;
  print("sum", sum);
  let xs = [1, 2, 3];
  xs[0] = big;
  print("xs", xs[0]);
  return 0;
}
