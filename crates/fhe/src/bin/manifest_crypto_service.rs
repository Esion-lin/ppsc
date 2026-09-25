//! Long-running OpenFHE/Shamir service used by `manifest_committee_daemon`.
//!
//! The wire protocol is one JSON request and response per line. Ciphertexts and
//! shares are intentionally never logged. Keeping this process alive also keeps
//! the demo committee's cryptographic context and keys stable for its lifetime.

use ppsc_fhe::real_backend::FheMpcBackend;
use ppsc_runtime::manifest::ManifestCryptoBackend;
use ppsc_runtime::manifest_crypto_process::{CryptoRequest, CryptoResponse};
use std::io::{self, BufRead, BufWriter, Write};

fn handle(backend: &FheMpcBackend, request: CryptoRequest) -> CryptoResponse {
    let result = match request {
        CryptoRequest::Evaluate { opcode, arguments } => arguments
            .into_iter()
            .map(|value| value.into_manifest())
            .collect::<Result<Vec<_>, _>>()
            .and_then(|arguments| backend.evaluate(&opcode, &arguments))
            .map(CryptoResponse::value),
        CryptoRequest::SecretBool { value } => value
            .into_manifest()
            .and_then(|value| backend.secret_bool(&value))
            .map(CryptoResponse::boolean),
        CryptoRequest::Pick { value } => value
            .into_manifest()
            .and_then(|value| backend.pick(&value))
            .map(CryptoResponse::value),
    };
    result.unwrap_or_else(|error| CryptoResponse::error(error.to_string()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = FheMpcBackend::new();
    if let Some(path) = std::env::var_os("MANIFEST_PUBLIC_KEY_PATH") {
        let path = std::path::PathBuf::from(path);
        let temporary = path.with_extension("pending");
        let key = backend.public_key().map_err(|_| "public key export failed")?;
        std::fs::write(&temporary, key)?;
        std::fs::rename(temporary, path)?;
    }
    eprintln!("OpenFHE BFV/Shamir service ready: pid={}", std::process::id());
    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let response = match serde_json::from_str::<CryptoRequest>(&line?) {
            Ok(request) => handle(&backend, request),
            Err(_) => CryptoResponse::error("invalid crypto request"),
        };
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}
