// Failures in one test must not stop the others.
contract fn half(x: i64) -> i64 effects()
  pre(x % 2 == 0, "偶数")
{
  return x / 2;
}

fn parse_id(text: string) -> i64 effects(throw) {
  let v = Id::parse(parse_json(text))?;
  return v.id;
}

schema Id { id: i64; }

test "通る" {
  expect(half(4) == 2);
}

test "expect が外れても続きを実行する" {
  let n = 3;
  expect(n == 4);
  expect(n == 5);
  print("最後まで来た");
}

test "pre 違反で止まっても次へ進む" {
  print(half(3));
}

test "送出された失敗" {
  print(parse_id("{}"));
}

test "ハンドラはブロックの外に漏れない" {
  with io = MemoryFs{ {"a", "1"} } {
    expect(read_file("a") == "1");
  }
  expect(read_file("a") == "1");
}
