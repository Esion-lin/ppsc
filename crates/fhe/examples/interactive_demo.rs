//! Interactive demo: type `deposit` / `transfer` / `balance` commands against the real
//! `FheMpcBackend` (BFV ciphertext balances + MPC secret comparison + threshold decrypt).
//!
//! Commands:
//!   create <account>                    open an account (encrypts a zero balance)
//!   deposit <account> <amount>          add (encrypted) balance to an account
//!   withdraw <account> <amount>         take (encrypted) balance out of an account
//!   transfer <from> <to> <amount>       conditional transfer (from's balance must exceed amount)
//!   balance <account>                   authorized opening (threshold decrypt)
//!   balance-all                         open every account
//!   help / quit

use ppsc_fhe::real_backend::FheMpcBackend;
use ppsc_runtime::{
    AssetId, FheAmount, FheBackend, HybridConversionBackend, MpcBackend, PlaintextBackend,
    PrivateAccountId,
};
use std::collections::HashMap;
use std::io::{self, Write};

fn account(name: &str) -> PrivateAccountId {
    let mut bytes = [0_u8; 32];
    for (i, b) in name.bytes().take(32).enumerate() {
        bytes[i] = b;
    }
    PrivateAccountId::from_bytes(bytes)
}

/// Amounts are stored as fixed-point integers: `units = value * SCALE` (2 decimal places).
const SCALE: u128 = 100;
const MAX_DECIMALS: usize = 2;
/// BFV plaintext modulus p = 998244353; a stored amount must stay below it.
const MAX_UNITS: u128 = 998244353;

/// Parse an amount such as `100`, `103.5`, or `103.50` into fixed-point units.
fn parse_amount(tok: &str) -> Result<u128, String> {
    let (int_part, frac_part) = match tok.split_once('.') {
        Some((i, f)) => (i, f),
        None => (tok, ""),
    };
    if frac_part.len() > MAX_DECIMALS {
        return Err(format!(
            "amount has more than {MAX_DECIMALS} decimal places: {tok}"
        ));
    }
    let int_val: u128 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().map_err(|_| format!("bad amount: {tok}"))?
    };
    let mut frac_val: u128 = 0;
    let mut scale_used: u128 = 1;
    for c in frac_part.chars() {
        let d = c.to_digit(10).ok_or_else(|| format!("bad amount: {tok}"))?;
        frac_val = frac_val * 10 + d as u128;
        scale_used *= 10;
    }
    while scale_used < SCALE {
        frac_val *= 10;
        scale_used *= 10;
    }
    let units = int_val * SCALE + frac_val;
    if units >= MAX_UNITS {
        return Err(format!(
            "amount too large: {} exceeds the BFV plaintext modulus (max {})",
            format_amount(units),
            format_amount(MAX_UNITS - 1)
        ));
    }
    Ok(units)
}

/// Format fixed-point units back into a decimal string.
fn format_amount(units: u128) -> String {
    let int = units / SCALE;
    let frac = units % SCALE;
    if frac == 0 {
        format!("{int}")
    } else {
        format!("{int}.{frac:02}")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = FheMpcBackend::new();
    let asset = AssetId::from_bytes([12; 20]);
    let auth = PlaintextBackend::authorization_for_tests();
    let mut balances: HashMap<PrivateAccountId, FheAmount> = HashMap::new();

    println!("PPSC interactive demo (real FHE + MPC)");
    println!(
        "  create <acc> | deposit <acc> <amt> | withdraw <acc> <amt> | transfer <from> <to> <amt> | balance <acc> | balance-all | quit"
    );

    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("> ");
        io::stdout().flush()?;
        line.clear();
        if stdin.read_line(&mut line)? == 0 {
            break;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        match tokens[0] {
            "create" => {
                if tokens.len() != 2 {
                    println!("usage: create <account>");
                    continue;
                }
                let acc = account(tokens[1]);
                if balances.contains_key(&acc) {
                    println!("{} already has an account", tokens[1]);
                    continue;
                }
                let ct = backend.encrypt_amount(0, acc, asset)?;
                balances.insert(acc, ct);
                println!("create: {} account opened (balance encrypted 0)", tokens[1]);
            }
            "deposit" => {
                if tokens.len() != 3 {
                    println!("usage: deposit <account> <amount>");
                    continue;
                }
                let acc = account(tokens[1]);
                let amount = match parse_amount(tokens[2]) {
                    Ok(a) => a,
                    Err(e) => {
                        println!("{e}");
                        continue;
                    }
                };
                match balances.get(&acc) {
                    None => {
                        let ct = backend.encrypt_amount(amount, acc, asset)?;
                        println!(
                            "deposit: {} += {} (encrypted, {} bytes)",
                            tokens[1],
                            format_amount(amount),
                            ct.as_bytes().len()
                        );
                        balances.insert(acc, ct);
                    }
                    Some(old) => {
                        let old_mpc = backend.fhe_to_mpc(old)?;
                        let amt_mpc = backend.share_amount(amount)?;
                        let new_mpc = backend.checked_add(&old_mpc, &amt_mpc)?;
                        let ct = backend.mpc_to_fhe(&new_mpc, acc, asset)?;
                        println!(
                            "deposit: {} += {} (H2S -> MPC add -> S2H)",
                            tokens[1],
                            format_amount(amount)
                        );
                        balances.insert(acc, ct);
                    }
                }
            }
            "transfer" => {
                if tokens.len() != 4 {
                    println!("usage: transfer <from> <to> <amount>");
                    continue;
                }
                let from = account(tokens[1]);
                let to = account(tokens[2]);
                let amount = match parse_amount(tokens[3]) {
                    Ok(a) => a,
                    Err(e) => {
                        println!("{e}");
                        continue;
                    }
                };

                let sender_ct = match balances.get(&from) {
                    Some(ct) => ct,
                    None => {
                        println!("transfer failed: {} has no balance", tokens[1]);
                        continue;
                    }
                };
                // Pre-transfer balance (authorized open, only to give the demo a clear result).
                let sender_before = backend.decrypt_for_owner(sender_ct, from, auth)?;

                let receiver_mpc = match balances.get(&to) {
                    Some(ct) => backend.fhe_to_mpc(ct)?,
                    None => {
                        println!(
                            "transfer failed: {} has no account (use `create {}` first)",
                            tokens[2], tokens[2]
                        );
                        continue;
                    }
                };
                let minimum_ct = backend.encrypt_amount(amount, from, asset)?;
                let amount_ct = backend.encrypt_amount(amount, from, asset)?;

                let sender_mpc = backend.fhe_to_mpc(sender_ct)?;
                let minimum_mpc = backend.fhe_to_mpc(&minimum_ct)?;
                let amount_mpc = backend.fhe_to_mpc(&amount_ct)?;

                let (sender_new, receiver_new) = backend.conditional_transfer_strictly_greater(
                    &sender_mpc,
                    &receiver_mpc,
                    &minimum_mpc,
                    &amount_mpc,
                )?;
                let sender_ct_new = backend.mpc_to_fhe(&sender_new, from, asset)?;
                let receiver_ct_new = backend.mpc_to_fhe(&receiver_new, to, asset)?;

                let sender_after = backend.decrypt_for_owner(&sender_ct_new, from, auth)?;
                balances.insert(from, sender_ct_new);
                balances.insert(to, receiver_ct_new);

                if sender_after < sender_before {
                    println!(
                        "transfer OK  {} -> {} {}: {} {} -> {} (secret compare {} > {} holds)",
                        tokens[1],
                        tokens[2],
                        format_amount(amount),
                        tokens[1],
                        format_amount(sender_before),
                        format_amount(sender_after),
                        tokens[1],
                        format_amount(amount)
                    );
                } else {
                    println!(
                        "transfer NO  {} -> {} {}: {} balance {} is not > {}, unchanged",
                        tokens[1],
                        tokens[2],
                        format_amount(amount),
                        tokens[1],
                        format_amount(sender_before),
                        format_amount(amount)
                    );
                }
            }
            "withdraw" => {
                if tokens.len() != 3 {
                    println!("usage: withdraw <account> <amount>");
                    continue;
                }
                let acc = account(tokens[1]);
                let amount = match parse_amount(tokens[2]) {
                    Ok(a) => a,
                    Err(e) => {
                        println!("{e}");
                        continue;
                    }
                };

                let ct = match balances.get(&acc) {
                    Some(ct) => ct,
                    None => {
                        println!(
                            "withdraw failed: {} has no account (use `create {}` first)",
                            tokens[1], tokens[1]
                        );
                        continue;
                    }
                };
                let before = backend.decrypt_for_owner(ct, acc, auth)?;
                if before < amount {
                    println!(
                        "withdraw NO  {} {}: balance {} is insufficient, unchanged",
                        tokens[1],
                        format_amount(amount),
                        format_amount(before)
                    );
                    continue;
                }
                let old_mpc = backend.fhe_to_mpc(ct)?;
                let amt_mpc = backend.share_amount(amount)?;
                let new_mpc = backend.checked_sub(&old_mpc, &amt_mpc)?;
                let ct_new = backend.mpc_to_fhe(&new_mpc, acc, asset)?;
                balances.insert(acc, ct_new);
                println!(
                    "withdraw OK  {} {}: {} -> {} (balance reduced)",
                    tokens[1],
                    format_amount(amount),
                    format_amount(before),
                    format_amount(before - amount)
                );
            }
            "balance" => {
                if tokens.len() != 2 {
                    println!("usage: balance <account>");
                    continue;
                }
                let acc = account(tokens[1]);
                match balances.get(&acc) {
                    Some(ct) => {
                        let v = backend.decrypt_for_owner(ct, acc, auth)?;
                        println!("{} = {}", tokens[1], format_amount(v));
                    }
                    None => println!(
                        "{} has no account (use `create {}` first)",
                        tokens[1], tokens[1]
                    ),
                }
            }
            "balance-all" => {
                let keys: Vec<PrivateAccountId> = balances.keys().copied().collect();
                if keys.is_empty() {
                    println!("(no accounts)");
                }
                for acc in keys {
                    let ct = balances.get(&acc).expect("balance");
                    let v = backend.decrypt_for_owner(ct, acc, auth)?;
                    println!(
                        "{} = {}",
                        String::from_utf8_lossy(
                            acc.as_bytes().split(|b| *b == 0).next().unwrap_or(&[])
                        ),
                        format_amount(v)
                    );
                }
            }
            "help" => {
                println!("create <acc> | deposit <acc> <amt> | withdraw <acc> <amt> | transfer <from> <to> <amt> | balance <acc> | balance-all | quit");
            }
            "quit" | "exit" => break,
            other => println!("unknown command: {other}"),
        }
    }

    Ok(())
}
