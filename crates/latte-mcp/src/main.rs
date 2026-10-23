//! `latte-mcp`: Dime's server, run by an AI assistant over standard input and output.
//!
//! `latte-mcp` works in the company the app chose for assistants; `latte-mcp --file
//! <path>` in that file instead.

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut file = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file" => file = args.next().map(PathBuf::from),
            "--version" => {
                println!("latte-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            _ => {
                eprintln!("usage: latte-mcp [--file <company.books.db>]");
                std::process::exit(2);
            }
        }
    }
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    if let Err(e) = latte_mcp::server::serve(file, stdin, stdout) {
        eprintln!("latte-mcp: {e}");
        std::process::exit(1);
    }
}
