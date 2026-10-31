namespace n {
test "名前空間の中" {
}
}
void g() {
  schema Inner {
    a: int;
  }
}
#pragma cc nonsense
[[cc::effects(ioo)]] void h();
