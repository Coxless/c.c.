// Step 4: effects. A function may only call functions whose effects it
// declares. `?` throws on failure, so it needs `throw`.
schema Config {
  port: u16    where 1 <= self <= 65535;
  host: string = "localhost";
}

// Pure apart from throwing: no io here, guaranteed by the signature.
fn parse_config(string text) -> Config effects(throw) {
  return Config::parse(parse_json(text))?;
}

fn load_config(string path) -> Config effects(io, throw) {
  return parse_config(read_file(path));
}

fn main() -> i32 effects(io, throw) {
  let c = parse_config(R"({"port": 8080})");
  print("port", c.port, "host", c.host);
  let missing = load_config("does-not-exist.json");
  print("ここには来ない", missing.port);
  return 0;
}
