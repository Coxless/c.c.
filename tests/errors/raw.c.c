schema User { id: u64 where self > 0; }
fn register(u: User) effects() {}
fn handle(body: Raw<Json>) effects() {
  register(body);
}
