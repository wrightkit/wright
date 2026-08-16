//! The external consumer binary (issue #61).

fn main() {
    let input = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: wright-consumer <input>");
        std::process::exit(2);
    });
    if let Err(message) = wright_consumer::run_consumer(&input) {
        eprintln!("wright-consumer: {message}");
        std::process::exit(1);
    }
}
