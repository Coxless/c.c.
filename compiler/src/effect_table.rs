//! The effects of standard library functions. Names are qualified, without template
//! arguments or inline namespaces. `::*` covers the rest of a class or namespace;
//! the most specific entry wins. A function that is not listed here and has no
//! `[[cc::effects]]` counts as `unsafe`, so a missing entry errs on the strict side.
//!
//! `std::bad_alloc` is not counted as `throw`: it would make `alloc` imply `throw`
//! everywhere. Allocation is `alloc`.

const TABLE: &[(&str, &str)] = &[
    // Moving things around.
    ("std::move", ""),
    ("std::forward", ""),
    ("std::addressof", ""),
    ("std::swap", ""),
    ("std::exchange", ""),
    ("std::min", ""),
    ("std::max", ""),
    ("std::clamp", ""),
    ("std::abs", ""),
    ("std::in_range", ""),
    ("std::cmp_equal", ""),
    ("std::cmp_not_equal", ""),
    ("std::cmp_less", ""),
    ("std::cmp_greater", ""),
    ("std::cmp_less_equal", ""),
    ("std::cmp_greater_equal", ""),
    ("std::size", ""),
    ("std::ssize", ""),
    ("std::begin", ""),
    ("std::end", ""),
    ("std::source_location::*", ""),
    ("std::initializer_list::*", ""),
    ("std::allocator::*", ""),
    ("std::char_traits::*", ""),
    ("std::pair::*", ""),
    ("std::tuple::*", ""),
    ("std::get", ""),
    ("std::nullopt_t::*", ""),
    // Comparisons between library types. std::operator+ on strings is below.
    ("std::operator==", ""),
    ("std::operator!=", ""),
    ("std::operator<", ""),
    ("std::operator<=", ""),
    ("std::operator>", ""),
    ("std::operator>=", ""),
    ("std::operator<=>", ""),
    ("__gnu_cxx::operator==", ""),
    ("__gnu_cxx::operator!=", ""),
    ("__gnu_cxx::operator<", ""),
    ("__gnu_cxx::operator<=", ""),
    ("__gnu_cxx::operator>", ""),
    ("__gnu_cxx::operator>=", ""),
    ("__gnu_cxx::operator<=>", ""),
    ("__gnu_cxx::operator-", ""),
    ("__gnu_cxx::operator+", ""),
    ("__gnu_cxx::__normal_iterator::*", ""),
    ("std::reverse_iterator::*", ""),
    // Values that may be absent.
    ("std::optional::*", ""),
    ("std::optional::value", "throw"),
    ("std::expected::*", ""),
    ("std::expected::value", "throw"),
    ("std::unexpected::*", ""),
    // Containers.
    ("std::vector::*", "alloc"),
    ("std::vector::~vector", ""),
    ("std::vector::size", ""),
    ("std::vector::empty", ""),
    ("std::vector::capacity", ""),
    ("std::vector::data", ""),
    ("std::vector::begin", ""),
    ("std::vector::end", ""),
    ("std::vector::cbegin", ""),
    ("std::vector::cend", ""),
    ("std::vector::rbegin", ""),
    ("std::vector::rend", ""),
    ("std::vector::front", ""),
    ("std::vector::back", ""),
    ("std::vector::operator[]", ""),
    ("std::vector::at", "throw"),
    ("std::vector::pop_back", ""),
    ("std::vector::clear", ""),
    ("std::array::*", ""),
    ("std::array::at", "throw"),
    ("std::span::*", ""),
    ("std::map::*", "alloc"),
    ("std::map::~map", ""),
    ("std::map::size", ""),
    ("std::map::empty", ""),
    ("std::map::find", ""),
    ("std::map::begin", ""),
    ("std::map::end", ""),
    ("std::map::at", "throw"),
    ("std::_Rb_tree_iterator::*", ""),
    ("std::_Rb_tree_const_iterator::*", ""),
    // Strings.
    ("std::basic_string::*", "alloc"),
    ("std::basic_string::~basic_string", ""),
    ("std::basic_string::size", ""),
    ("std::basic_string::length", ""),
    ("std::basic_string::empty", ""),
    ("std::basic_string::data", ""),
    ("std::basic_string::c_str", ""),
    ("std::basic_string::begin", ""),
    ("std::basic_string::end", ""),
    ("std::basic_string::cbegin", ""),
    ("std::basic_string::cend", ""),
    ("std::basic_string::front", ""),
    ("std::basic_string::back", ""),
    ("std::basic_string::operator[]", ""),
    ("std::basic_string::find", ""),
    ("std::basic_string::rfind", ""),
    ("std::basic_string::find_first_of", ""),
    ("std::basic_string::find_first_not_of", ""),
    ("std::basic_string::find_last_of", ""),
    ("std::basic_string::find_last_not_of", ""),
    ("std::basic_string::compare", ""),
    ("std::basic_string::starts_with", ""),
    ("std::basic_string::ends_with", ""),
    ("std::basic_string::operator basic_string_view", ""),
    ("std::basic_string::at", "throw"),
    ("std::basic_string::substr", "alloc,throw"),
    ("std::basic_string_view::*", ""),
    ("std::basic_string_view::at", "throw"),
    ("std::operator+", "alloc"),
    ("std::operator\"\"s", "alloc"),
    ("std::to_string", "alloc"),
    ("std::stoi", "alloc,throw"),
    ("std::stol", "alloc,throw"),
    ("std::stoll", "alloc,throw"),
    ("std::stoul", "alloc,throw"),
    ("std::stoull", "alloc,throw"),
    ("std::stod", "alloc,throw"),
    // Input and output.
    ("std::operator<<", "io"),
    ("std::operator>>", "io"),
    ("std::endl", "io"),
    ("std::flush", "io"),
    ("std::getline", "io"),
    ("std::basic_ostream::*", "io"),
    ("std::basic_istream::*", "io"),
    ("std::basic_iostream::*", "io"),
    ("std::basic_ios::*", "io"),
    ("std::ios_base::*", "io"),
    ("std::basic_ifstream::*", "io"),
    ("std::basic_ofstream::*", "io"),
    ("std::basic_fstream::*", "io"),
    ("std::basic_filebuf::*", "io"),
    ("std::basic_streambuf::*", "io"),
    // Writing into memory is not input or output, but it does allocate.
    ("std::basic_ostringstream::*", "alloc"),
    ("std::basic_istringstream::*", "alloc"),
    ("std::basic_stringstream::*", "alloc"),
    // Exceptions.
    ("std::exception::*", ""),
    ("std::runtime_error::*", "alloc"),
    ("std::logic_error::*", "alloc"),
    ("std::out_of_range::*", "alloc"),
    ("std::invalid_argument::*", "alloc"),
    ("std::domain_error::*", "alloc"),
    ("std::range_error::*", "alloc"),
    ("std::overflow_error::*", "alloc"),
    // Ownership.
    ("std::unique_ptr::*", ""),
    ("std::make_unique", "alloc"),
    ("std::shared_ptr::*", ""),
    ("std::make_shared", "alloc"),
    // Time and randomness.
    ("std::chrono::*", ""),
    ("std::chrono::system_clock::now", "nondet"),
    ("std::chrono::steady_clock::now", "nondet"),
    ("std::chrono::_V2::system_clock::now", "nondet"),
    ("std::chrono::_V2::steady_clock::now", "nondet"),
    ("std::random_device::*", "nondet"),
    ("std::rand", "nondet"),
    ("rand", "nondet"),
    ("time", "nondet"),
    // JSON, which is built on all of the above.
    ("nlohmann::*", "alloc,throw"),
    ("nlohmann::basic_json::~basic_json", ""),
];

/// The effects listed for a qualified name, or `None` when it is not listed.
pub fn lookup(qualified: &str) -> Option<Vec<String>> {
    let split = |list: &str| list.split(',').filter(|e| !e.is_empty()).map(str::to_string).collect();
    if let Some((_, list)) = TABLE.iter().find(|(n, _)| *n == qualified) {
        return Some(split(list));
    }
    let mut prefix = qualified;
    while let Some(i) = prefix.rfind("::") {
        prefix = &prefix[..i];
        let wildcard = format!("{prefix}::*");
        if let Some((_, list)) = TABLE.iter().find(|(n, _)| *n == wildcard) {
            return Some(split(list));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_specific_entry_wins() {
        assert_eq!(lookup("std::vector::size"), Some(vec![]));
        assert_eq!(lookup("std::vector::push_back"), Some(vec!["alloc".to_string()]));
        assert_eq!(lookup("std::vector::at"), Some(vec!["throw".to_string()]));
        assert_eq!(lookup("fopen"), None);
        assert_eq!(lookup("std::fopen"), None);
    }
}
