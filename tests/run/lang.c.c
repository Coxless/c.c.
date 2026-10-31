// match, try / catch, enum, open schema, invariants that call functions,
// loops over refined vectors, and literal defaults of refined types.
enum Color { red, green }

extern fn is_level(s: string) -> bool effects();
cpp {
inline bool is_level(std::string s) { return s == "info" || s == "debug"; }
}
type Level = string matches is_level;
type Port  = u16 where self >= 1;

schema Paint {
  color:   Color = green;
  finish:  enum { matte, gloss, default }?;
  level:   Level = "info";
  port:    Port = 8080;
  sizes:   vec<i64> where len(self) >= 1;
  invariant(total(sizes) < 100, "合計は100未満");
  invariant(color != red || finish != gloss, "赤に光沢は不可");
}

open schema Loose {
  id: i64;
}

fn total(xs: vec<i64>) -> i64 effects() {
  let sum = 0;
  for x in xs {
    sum += x;
  }
  return sum;
}

fn is_red(c: Color) -> bool effects() {
  return c == red;
}

fn describe(c: Color?) effects(io) {
  match c {
    some(x) { print("色", x, is_red(x)); }
    none { print("色なし"); }
  }
}

fn checked(n: i64) -> i64 effects(io) {
  match Port::try_from(n) {
    ok(p) { return p + 1; }
    err(e) {
      print(e);
      return 0;
    }
  }
}

fn reads(path: string) -> string effects(io) {
  let text = "(なし)";
  try {
    text = read_file(path);
  } catch message {
    print("捕まえた:", message);
  }
  return text;
}

fn main(args: vec<string>) -> i32 effects(io) {
  print("引数", args);
  let r = Paint::parse(parse_json(R"({"sizes": [1, 2], "finish": "default"})"));
  match r {
    ok(p) {
      print(p);
      print(p.color == green, p.finish == default, p.sizes[1]);
      for s in p.sizes {
        print(s);
      }
      describe(p.color);
    }
    err(e) { print(e); }
  }
  print(Paint::parse(parse_json(R"({"sizes": [99, 1], "color": "red", "finish": "gloss"})")));
  print(Paint::parse(parse_json(R"({"sizes": [], "color": "blue", "finish": 1, "level": "x", "extra": true})")));
  print(Loose::parse(parse_json(R"({"id": 1, "extra": true})")));
  describe(none);
  describe(Color::red);
  print(checked(0), checked(7));
  print(reads("does-not-exist"));
  return 0;
}
