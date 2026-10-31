test "a" {
  with log = MemoryFs{} {
  }
  with io = RealFs{} {
  }
}
