fn helper() {}
fn pure_fn(x: i64) -> i64 effects(io) pre(x > 0) {
  helper();
  print(x);
  cpp { std::puts("hi"); }
  let y = Foo::bar()?;
  return x + old(x);
}
