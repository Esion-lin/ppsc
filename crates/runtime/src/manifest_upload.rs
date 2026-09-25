//! Authenticated opaque-input upload protocol shared by clients and committee nodes.

use crate::manifest::{ManifestRuntimeError, ManifestValue};
use ppsc_compiler::ValueType;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use tiny_keccak::{Hasher, Keccak};

pub const MAX_UPLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadValueType {
    FheUint,
    Sint,
}

impl UploadValueType {
    pub const fn manifest_type(self) -> ValueType {
        match self {
            Self::FheUint => ValueType::FheUint,
            Self::Sint => ValueType::Sint,
        }
    }

    const fn tag(self) -> u8 {
        match self {
            Self::FheUint => 1,
            Self::Sint => 2,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SignedUploadRequest {
    pub owner: String,
    pub value_type: UploadValueType,
    /// Opaque backend ciphertext or serialized share bundle, hex encoded.
    pub payload: String,
    pub nonce: u64,
    pub deadline: u64,
    pub signature: String,
}

#[derive(Serialize, Deserialize)]
pub struct UploadResponse {
    pub data_id: String,
    pub status: String,
}

pub struct ValidatedUpload {
    pub owner: [u8; 20],
    pub data_id: [u8; 32],
    pub commitment: [u8; 32],
    pub value: ManifestValue,
}

impl SignedUploadRequest {
    pub fn validate_payload(
        &self,
        control: [u8; 20],
        contract_id: [u8; 32],
    ) -> Result<ValidatedUpload, ManifestRuntimeError> {
        let owner = decode_fixed::<20>(&self.owner)?;
        let payload = decode_hex(&self.payload)?;
        if payload.is_empty() || payload.len() > MAX_UPLOAD_BYTES {
            return Err(ManifestRuntimeError::InvalidValue);
        }
        let commitment = keccak(&payload);
        let data_id = upload_data_id(
            control,
            contract_id,
            owner,
            self.value_type,
            commitment,
            self.nonce,
        );
        let value = ManifestValue::from_backend_parts(self.value_type.manifest_type(), payload)?;
        Ok(ValidatedUpload {
            owner,
            data_id,
            commitment,
            value,
        })
    }

    pub fn signing_digest(
        &self,
        control: [u8; 20],
        contract_id: [u8; 32],
    ) -> Result<[u8; 32], ManifestRuntimeError> {
        let validated = self.validate_payload(control, contract_id)?;
        Ok(upload_signing_digest(
            control,
            contract_id,
            validated.owner,
            self.value_type,
            validated.commitment,
            validated.data_id,
            self.nonce,
            self.deadline,
        ))
    }
}

pub fn upload_data_id(
    control: [u8; 20],
    contract_id: [u8; 32],
    owner: [u8; 20],
    value_type: UploadValueType,
    commitment: [u8; 32],
    nonce: u64,
) -> [u8; 32] {
    keccak_parts(&[
        b"PPSC_MANIFEST_INPUT_ID_V1",
        &control,
        &contract_id,
        &owner,
        &[value_type.tag()],
        &commitment,
        &nonce.to_be_bytes(),
    ])
}

#[allow(clippy::too_many_arguments)]
pub fn upload_signing_digest(
    control: [u8; 20],
    contract_id: [u8; 32],
    owner: [u8; 20],
    value_type: UploadValueType,
    commitment: [u8; 32],
    data_id: [u8; 32],
    nonce: u64,
    deadline: u64,
) -> [u8; 32] {
    keccak_parts(&[
        b"PPSC_MANIFEST_UPLOAD_AUTH_V1",
        &control,
        &contract_id,
        &owner,
        &[value_type.tag()],
        &commitment,
        &data_id,
        &nonce.to_be_bytes(),
        &deadline.to_be_bytes(),
    ])
}

pub fn read_http_upload(stream: &mut TcpStream) -> Result<SignedUploadRequest, String> {
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    let header_end = loop {
        let count = stream.read(&mut buffer).map_err(|_| "read failed")?;
        if count == 0 {
            return Err("incomplete request".to_owned());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > MAX_UPLOAD_BYTES + 8192 {
            return Err("request too large".to_owned());
        }
        if let Some(index) = find_subslice(&bytes, b"\r\n\r\n") {
            break index + 4;
        }
    };
    let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|_| "invalid headers")?;
    let request_line = headers.lines().next().ok_or("missing request line")?;
    if request_line != "POST /v1/inputs HTTP/1.1" {
        return Err("expected POST /v1/inputs".to_owned());
    }
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or("missing content-length")?;
    if content_length > MAX_UPLOAD_BYTES {
        return Err("request too large".to_owned());
    }
    while bytes.len() < header_end + content_length {
        let count = stream.read(&mut buffer).map_err(|_| "read failed")?;
        if count == 0 {
            return Err("incomplete body".to_owned());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    serde_json::from_slice(&bytes[header_end..header_end + content_length])
        .map_err(|_| "invalid json".to_owned())
}

pub fn write_http_json<T: Serialize>(
    stream: &mut TcpStream,
    status: &str,
    value: &T,
) -> std::io::Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}

pub fn post_upload(url: &str, request: &SignedUploadRequest) -> Result<UploadResponse, String> {
    let address = url
        .strip_prefix("http://")
        .ok_or("UPLOAD_URL must use http://")?;
    let (authority, path) = address.split_once('/').unwrap_or((address, "v1/inputs"));
    let path = format!("/{path}");
    if path != "/v1/inputs" {
        return Err("UPLOAD_URL path must be /v1/inputs".to_owned());
    }
    let body = serde_json::to_vec(request).map_err(|_| "cannot encode request")?;
    let mut stream = TcpStream::connect(authority).map_err(|error| error.to_string())?;
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .and_then(|_| stream.write_all(&body))
    .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    let header_end = find_subslice(&response, b"\r\n\r\n").ok_or("invalid HTTP response")? + 4;
    let headers = std::str::from_utf8(&response[..header_end]).map_err(|_| "invalid response")?;
    if !headers.starts_with("HTTP/1.1 202") && !headers.starts_with("HTTP/1.1 200") {
        return Err(String::from_utf8_lossy(&response[header_end..]).into_owned());
    }
    serde_json::from_slice(&response[header_end..]).map_err(|_| "invalid response json".to_owned())
}

pub fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(2 + bytes.len() * 2);
    output.push_str("0x");
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

pub fn decode_fixed<const N: usize>(value: &str) -> Result<[u8; N], ManifestRuntimeError> {
    decode_hex(value)?
        .try_into()
        .map_err(|_| ManifestRuntimeError::InvalidValue)
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ManifestRuntimeError> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if value.len() % 2 != 0 {
        return Err(ManifestRuntimeError::InvalidValue);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| ManifestRuntimeError::InvalidValue)
        })
        .collect()
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    keccak_parts(&[bytes])
}

fn keccak_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    for part in parts {
        hasher.update(part);
    }
    let mut output = [0_u8; 32];
    hasher.finalize(&mut output);
    output
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|part| part == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_binds_nonce_deadline_and_representation() {
        let mut request = SignedUploadRequest {
            owner: hex(&[3_u8; 20]),
            value_type: UploadValueType::FheUint,
            payload: "0x0102".to_owned(),
            nonce: 1,
            deadline: 9,
            signature: String::new(),
        };
        let first = request.signing_digest([1; 20], [2; 32]).expect("digest");
        request.nonce = 2;
        assert_ne!(
            first,
            request.signing_digest([1; 20], [2; 32]).expect("digest")
        );
    }
}
