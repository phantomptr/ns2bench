mod uf2;

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = (|| -> Result<(), String> {
        if args.len() != 3 || args[0] != "elf2uf2" {
            return Err("usage: ns2bench-tools elf2uf2 input.elf output.uf2".into());
        }
        let input = std::fs::read(&args[1]).map_err(|e| e.to_string())?;
        let output = uf2::convert(&input)?;
        std::fs::write(&args[2], &output).map_err(|e| e.to_string())?;
        println!(
            "{}: {} blocks, {} KB of flash",
            args[2],
            output.len() / 512,
            output.len() / 512 / 4
        );
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("NS2Bench tools: {e}");
        std::process::exit(1);
    }
}
