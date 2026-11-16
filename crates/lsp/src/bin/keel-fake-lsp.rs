//! The tests' fake language server as a program, to test starting and
//! stopping a real process.

fn main() {
    eprintln!("fake server ready");
    let code = lsp::fake::run(std::io::stdin(), std::io::stdout());
    std::process::exit(code);
}
