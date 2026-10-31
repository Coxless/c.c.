type Port = u16 where 1 <= self <= 65535;
fn connect(p: Port) effects() {}
fn main() effects() {
  let n = 8079;
  connect(n + 1);
  connect(n);
  let p: Port = 8080;
  connect(p);
}
