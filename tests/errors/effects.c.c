extern fn parse(string s) -> Json effects();
extern fn load(string p) -> string effects(io, throw);

fn config() -> Json effects(io, throw) {
  return parse(load("a.json"));
}

fn bad(string s) -> Json effects() {
  load("x");
  return parse(s);
}
