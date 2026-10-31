// Statements, expressions and name mangling.
type Percent = i64 where 0 <= self <= 100;

schema Item {
  name:  string;
  price: i64 where self >= 0;
}

fn register(p: Percent) -> i64 effects() {
  return p * 2;
}

fn total(items: vec<Item>) -> i64 effects() {
  let sum = 0;
  for it in items {
    sum += it.price;
  }
  return sum;
}

fn main() effects(io, throw) {
  print("register", register(40));
  let xs: vec<i64> = [3, 1, 2];
  let n = 0;
  for i in 0..len(xs) {
    if xs[i] > 1 && !(xs[i] == 3) {
      n += 10;
    } else if xs[i] == 3 {
      n += 100;
    } else {
      n += 1;
    }
  }
  print("n", n);
  let k = 3;
  while k > 0 {
    k -= 1;
  }
  print("k", k, -5 + 2 * 3, (1 + 2) * 3, 7 % 4, 1 < 2 <= 2);
  let p = Percent::try_from(150);
  print("percent", p);
  let cart = Item::parse(parse_json(R"({"name": "x", "price": 5})"))?;
  let items = [cart, cart];
  print("total", total(items));
}
