fn helper() {}
fn pure_fn(i64 x) -> i64 effects(io) pre(x > 0) {
  helper();
  print(x);
  cpp { std::puts("hi"); }
  let y = Foo::bar()?;
  return x + old(x);
}
