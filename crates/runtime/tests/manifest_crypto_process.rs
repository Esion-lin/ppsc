use ppsc_runtime::manifest::{ManifestCryptoBackend, PlaintextManifestBackend};
use ppsc_runtime::manifest_crypto_process::ProcessManifestBackend;

#[test]
fn long_running_process_backend_evaluates_manifest_operator() {
    let program = env!("CARGO_BIN_EXE_manifest_crypto_plaintext_service");
    let backend = ProcessManifestBackend::spawn(program, &[]).expect("spawn crypto service");
    let output = backend
        .evaluate(
            "fhe.add",
            &[
                PlaintextManifestBackend::fhe_uint(20),
                PlaintextManifestBackend::fhe_uint(22),
            ],
        )
        .expect("evaluate");
    assert_eq!(PlaintextManifestBackend::open_for_tests(&output), Ok(42));
}
