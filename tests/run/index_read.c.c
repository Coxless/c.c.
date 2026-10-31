// Reading outside a vector is reported against the caller, on the .c.c line.
schema S {
  items: vec<i64> where len(self) >= 1;
}

fn main() -> i32 effects(io, throw) {
  let v = [10, 20, 30];
  print(v[2]);
  let s = S::parse(parse_json(R"({"items": [1, 2]})"))?;
  print(s.items[1]);
  let i = 3;
  print(v[i]);
  print("届かない");
  return 0;
}
