//! NS2Bench Bridge: one static binary for Linux USB-gadget boards (Luckfox, Raspberry Pi).
//!   ns2bench-board hidd [--bind A] [--port N] [--token-file F] [--pad D] [--kbd D] [--mouse D] [--kmsg D]
//!   ns2bench-board pro2 [/dev/ffs-pro2]
//!   ns2bench-board version
//! Protocol compatibility is covered by synthetic golden fixtures and daemon lifecycle tests.

mod aes;
mod hidd;
mod logbuf;
mod pro1;
mod pro2;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("hidd") => hidd::main(&args[1..]),
        Some("pro2") => pro2::main(args.get(1).map(String::as_str).unwrap_or("/dev/ffs-pro2")),
        Some("version") | Some("--version") => {
            println!("NS2Bench Bridge {} (ns2bench-board)", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        _ => {
            eprintln!("usage: ns2bench-board hidd [options] | pro2 [/dev/ffs-pro2] | version");
            std::process::exit(2);
        }
    };
    if let Err(e) = r {
        eprintln!("NS2Bench Bridge: {e}");
        std::process::exit(1);
    }
}
