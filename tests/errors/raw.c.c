schema User { id: u64 where self > 0; }
fn register(User u) effects() {}
fn handle(Raw<Json> body) effects() {
  register(body);
}
