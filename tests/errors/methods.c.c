schema S { port: u16; }
fn pure_at() -> i64 effects() {
  let v: vec<i64> = [];
  return v.at(3);
}
fn unwraps() -> u16 effects(io) {
  let r = S::parse(parse_json("{}"));
  return r.value().port;
}
fn fine(s: S) effects(io) {
  print(s.to_json());
}
