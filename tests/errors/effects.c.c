extern fn parse(s: string) -> Json effects();
extern fn load(p: string) -> string effects(io, throw);

fn config() -> Json effects(io, throw) {
  return parse(load("a.json"));
}

fn bad(s: string) -> Json effects() {
  load("x");
  return parse(s);
}
