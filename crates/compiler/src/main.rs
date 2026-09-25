use ppsc_compiler::{compile_file, manifest_from_path, write_solidity_gateway};
use std::env;
use std::path::PathBuf;

fn usage() -> &'static str {
    "usage: ppsc build <contract.ppsc> [--out <directory>] [--sol-out <directory>]"
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some("build") {
        return Err(usage().into());
    }
    let source = args.next().map(PathBuf::from).ok_or_else(|| usage())?;
    let mut output = PathBuf::from("target/ppsc");
    let mut solidity_output = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--out" => {
                output = args.next().map(PathBuf::from).ok_or_else(|| usage())?;
            }
            "--sol-out" => {
                solidity_output = Some(args.next().map(PathBuf::from).ok_or_else(|| usage())?);
            }
            _ => return Err(usage().into()),
        }
    }
    let generated = compile_file(&source, &output)?;
    println!("compiled {}", source.display());
    println!("artifacts {}", generated.display());
    if let Some(solidity_output) = solidity_output {
        let manifest = manifest_from_path(&generated.join("manifest.json"))?;
        let gateway = write_solidity_gateway(&manifest, &solidity_output)?;
        println!("gateway {}", gateway.display());
    }
    Ok(())
}
