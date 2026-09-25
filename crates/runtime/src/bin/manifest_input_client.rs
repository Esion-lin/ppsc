//! User-side client for authenticated confidential input uploads.

use ppsc_runtime::manifest::PlaintextManifestBackend;
use ppsc_runtime::manifest_upload::{
    decode_fixed, hex, post_upload, SignedUploadRequest, UploadValueType,
};
use std::env;
use std::error::Error;
use std::fs;
use std::process::Command;

fn required(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("missing environment variable {name}").into())
}

fn cast(args: &[String]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("cast").args(args).output()?;
    if !output.status.success() {
        return Err(format!("cast failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn usage() -> &'static str {
    "usage:\n  manifest_input_client dev-fhe <amount> <nonce> <deadline>\n  manifest_input_client fhe-file <ciphertext-file> <nonce> <deadline>\n  manifest_input_client ss-file <share-bundle-file> <nonce> <deadline>"
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).ok_or_else(|| usage().to_owned())?;
    let source = args.get(2).ok_or_else(|| usage().to_owned())?;
    let nonce = args
        .get(3)
        .ok_or_else(|| usage().to_owned())?
        .parse::<u64>()?;
    let deadline = args
        .get(4)
        .ok_or_else(|| usage().to_owned())?
        .parse::<u64>()?;

    let (value_type, payload) = match mode.as_str() {
        "dev-fhe" => {
            let amount = source.parse::<u128>()?;
            let value = PlaintextManifestBackend::fhe_uint(amount);
            (UploadValueType::FheUint, value.backend_bytes().to_vec())
        }
        "fhe-file" => (UploadValueType::FheUint, fs::read(source)?),
        "ss-file" => (UploadValueType::Sint, fs::read(source)?),
        _ => return Err(usage().into()),
    };

    let private_key = required("USER_PRIVATE_KEY")?;
    let owner = cast(&[
        "wallet".to_owned(),
        "address".to_owned(),
        "--private-key".to_owned(),
        private_key.clone(),
    ])?;
    let control = decode_fixed::<20>(&required("CONTROL")?)?;
    let contract_id = decode_fixed::<32>(&required("CONTRACT_ID")?)?;
    let mut request = SignedUploadRequest {
        owner: owner.clone(),
        value_type,
        payload: hex(&payload),
        nonce,
        deadline,
        signature: String::new(),
    };
    let digest = hex(&request.signing_digest(control, contract_id)?);
    request.signature = cast(&[
        "wallet".to_owned(),
        "sign".to_owned(),
        "--no-hash".to_owned(),
        "--private-key".to_owned(),
        private_key,
        digest,
    ])?;

    let response = post_upload(&required("UPLOAD_URL")?, &request)?;
    println!(
        "input accepted: owner={owner} dataId={} status={}",
        response.data_id, response.status
    );
    Ok(())
}
