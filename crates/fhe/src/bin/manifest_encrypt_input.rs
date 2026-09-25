//! User-side BFV encryption. Only a public key is needed; output is a binary ciphertext.
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 {
        return Err("usage: manifest_encrypt_input <public-key-file> <amount> <ciphertext-file> | --validate <public-key-file> <ciphertext-file>".into());
    }
    if args[1] == "--validate" {
        if !ppsc_fhe::validate_public_ciphertext(&fs::read(&args[2])?, &fs::read(&args[3])?) {
            return Err("ciphertext encoding, key or context mismatch".into());
        }
        println!("BFV ciphertext validated");
        return Ok(());
    }
    let amount = args[2].parse::<u128>()?;
    let ciphertext = ppsc_fhe::encrypt_public_amount(&fs::read(&args[1])?, amount)
        .ok_or("BFV encryption failed: invalid public key or amount outside 0..499122176")?;
    fs::write(&args[3], &ciphertext)?;
    println!("BFV ciphertext written: {} bytes", ciphertext.len());
    Ok(())
}
