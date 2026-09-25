//! Real-password demo of deposit / query / transfer / query using the `FheMpcBackend`
//! (BFV Multiparty ciphertexts + Fr Shamir sharings + MPC comparison).
//!
//! Prints the ciphertext bytes at each step so the "encrypted / secret-shared, never plaintext"
//! flow is visible. Runs all parties in one process.

use ppsc_fhe::real_backend::FheMpcBackend;
use ppsc_runtime::{
    AssetId, FheAmount, FheBackend, HybridConversionBackend, MpcBackend, PlaintextBackend,
    PrivateAccountId,
};

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(2 + bytes.len() * 2);
    output.push_str("0x");
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut output, "{byte:02x}").expect("write");
    }
    output
}

fn show_ciphertext(label: &str, ct: &FheAmount) {
    let bytes = ct.as_bytes();
    println!(
        "{label}: {} bytes, first 24 bytes {}",
        bytes.len(),
        hex(&bytes[..24.min(bytes.len())])
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = FheMpcBackend::new();
    let sender = PrivateAccountId::from_bytes([10; 32]);
    let receiver = PrivateAccountId::from_bytes([11; 32]);
    let asset = AssetId::from_bytes([12; 20]);
    let auth = PlaintextBackend::authorization_for_tests();

    // 1. deposit: encrypt balances (sender 100, receiver 20, minimum 50, amount 30).
    println!("== deposit (BFV encrypt balances) ==");
    let sender_ct = backend.encrypt_amount(100, sender, asset)?;
    let receiver_ct = backend.encrypt_amount(20, receiver, asset)?;
    let minimum_ct = backend.encrypt_amount(50, sender, asset)?;
    let amount_ct = backend.encrypt_amount(30, sender, asset)?;
    show_ciphertext("sender ciphertext  ", &sender_ct);
    show_ciphertext("receiver ciphertext", &receiver_ct);

    // 2. query (authorized opening, multiparty decrypt).
    println!("\n== query before (authorized opening) ==");
    println!(
        "sender  = {}",
        backend.decrypt_for_owner(&sender_ct, sender, auth)?
    );
    println!(
        "receiver= {}",
        backend.decrypt_for_owner(&receiver_ct, receiver, auth)?
    );

    // 3. transfer: H2S -> MPC(gt, conditional_sub_add) -> S2H.
    println!("\n== transfer ==");
    println!("H2S: multiparty-decrypt + Shamir-share each balance");
    let sender_mpc = backend.fhe_to_mpc(&sender_ct)?;
    let receiver_mpc = backend.fhe_to_mpc(&receiver_ct)?;
    let minimum_mpc = backend.fhe_to_mpc(&minimum_ct)?;
    let amount_mpc = backend.fhe_to_mpc(&amount_ct)?;
    println!("MPC: secret comparison sender(100) > minimum(50), then conditional sub/add");
    let (sender_new_mpc, receiver_new_mpc) = backend.conditional_transfer_strictly_greater(
        &sender_mpc,
        &receiver_mpc,
        &minimum_mpc,
        &amount_mpc,
    )?;
    println!("S2H: reconstruct sharing + BFV re-encrypt");
    let new_sender = backend.mpc_to_fhe(&sender_new_mpc, sender, asset)?;
    let new_receiver = backend.mpc_to_fhe(&receiver_new_mpc, receiver, asset)?;
    show_ciphertext("sender' ciphertext ", &new_sender);
    show_ciphertext("receiver' ciphertext", &new_receiver);

    // 4. query after.
    println!("\n== query after (authorized opening) ==");
    println!(
        "sender  = {}",
        backend.decrypt_for_owner(&new_sender, sender, auth)?
    );
    println!(
        "receiver= {}",
        backend.decrypt_for_owner(&new_receiver, receiver, auth)?
    );

    Ok(())
}
