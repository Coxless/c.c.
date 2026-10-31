type Port = u16 where self >= 1;
fn a(n: i64) effects() {
  match Port::try_from(n) {
    ok(p) {}
  }
}
fn b(n: i64) effects() {
  match n {
    ok(p) {}
    err(e) {}
  }
}
fn c(n: i64?) effects() {
  match n {
    ok(p) {}
    none {}
  }
}
fn d(n: i64) effects() {
  let p: Port = Port::try_from(n);
  let q = n?;
}
fn e() effects() {
  try {
    read_file("x");
  } catch m {
  }
}
