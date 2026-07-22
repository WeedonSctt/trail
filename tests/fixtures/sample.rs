/// Sample Rust source file for syntax-highlighting tests.
///
/// Used by `tests/state_tests.rs` to verify that the text provider
/// produces `Highlighted` output for `.rs` files.

fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn main() {
    println!("{}", greet("Trail"));
}
