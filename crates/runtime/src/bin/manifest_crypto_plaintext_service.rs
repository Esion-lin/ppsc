//! Portable development implementation of the daemon crypto-process protocol.
//! It exists only for local integration tests; use `ppsc-fhe`'s
//! `manifest_crypto_service` for real OpenFHE/Shamir values.

use ppsc_runtime::manifest::{ManifestCryptoBackend, PlaintextManifestBackend};
use ppsc_runtime::manifest_crypto_process::{CryptoRequest, CryptoResponse};
use std::io::{self, BufRead, BufWriter, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = PlaintextManifestBackend;
    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let response = match serde_json::from_str::<CryptoRequest>(&line?) {
            Ok(CryptoRequest::Evaluate { opcode, arguments }) => arguments
                .into_iter()
                .map(|value| value.into_manifest())
                .collect::<Result<Vec<_>, _>>()
                .and_then(|arguments| backend.evaluate(&opcode, &arguments))
                .map(CryptoResponse::value),
            Ok(CryptoRequest::SecretBool { value }) => value
                .into_manifest()
                .and_then(|value| backend.secret_bool(&value))
                .map(CryptoResponse::boolean),
            Ok(CryptoRequest::Pick { value }) => value
                .into_manifest()
                .and_then(|value| backend.pick(&value))
                .map(CryptoResponse::value),
            Err(_) => {
                serde_json::to_writer(
                    &mut stdout,
                    &CryptoResponse::error("invalid crypto request"),
                )?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
                continue;
            }
        }
        .unwrap_or_else(|error| CryptoResponse::error(error.to_string()));
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}
