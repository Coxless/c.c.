// Step 1: a refined type.
type Port = u16 where 1 <= self <= 65535;

fn main() effects(io) {
  let http: Port = 8080;
  print("literal:", http);
  print("try_from(443):", Port::try_from(443));
  print("try_from(0):", Port::try_from(0));
}
