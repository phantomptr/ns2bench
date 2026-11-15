fn main() {
    #[cfg(unix)]
    let result = ns2bench_netd::runtime::main(std::env::args().skip(1).collect());
    #[cfg(not(unix))]
    let result: Result<(), String> =
        Err("netd requires macOS or Linux; use a Linux board for the console network".into());
    if let Err(error) = result {
        eprintln!("NS2Bench netd: {error}");
        std::process::exit(1);
    }
}
