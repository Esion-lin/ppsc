//! JSON-lines bridge between the committee daemon and a long-running MPC/FHE process.
//!
//! The child remains alive for the daemon lifetime, so committee keys and preprocessing
//! state are not regenerated for every operator. Secret payloads only cross local pipes
//! and are never included in logs.

use crate::manifest::{ManifestCryptoBackend, ManifestRuntimeError, ManifestValue};
use ppsc_compiler::ValueType;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CryptoRequest {
    Evaluate {
        opcode: String,
        arguments: Vec<WireValue>,
    },
    SecretBool {
        value: WireValue,
    },
    Pick {
        value: WireValue,
    },
}

#[derive(Serialize, Deserialize)]
pub struct CryptoResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<WireValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boolean: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl CryptoResponse {
    pub fn value(value: ManifestValue) -> Self {
        Self {
            value: Some(WireValue::from_manifest(&value)),
            boolean: None,
            error: None,
        }
    }

    pub const fn boolean(value: bool) -> Self {
        Self {
            value: None,
            boolean: Some(value),
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            value: None,
            boolean: None,
            error: Some(message.into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct WireValue {
    pub value_type: ValueType,
    pub payload: String,
}

impl WireValue {
    pub fn from_manifest(value: &ManifestValue) -> Self {
        Self {
            value_type: value.value_type(),
            payload: hex(value.backend_bytes()),
        }
    }

    pub fn into_manifest(self) -> Result<ManifestValue, ManifestRuntimeError> {
        ManifestValue::from_backend_parts(self.value_type, decode_hex(&self.payload)?)
    }
}

struct ProcessIo {
    _child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

pub struct ProcessManifestBackend {
    io: Mutex<ProcessIo>,
}

impl ProcessManifestBackend {
    /// Detect an exited crypto process even while no chain tasks are arriving.
    pub fn ensure_running(&self) -> Result<(), ManifestRuntimeError> {
        let mut io = self.io.lock().map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        match io._child.try_wait() {
            Ok(None) => Ok(()),
            _ => Err(ManifestRuntimeError::StateUnavailable),
        }
    }

    pub fn spawn(program: &str, arguments: &[String]) -> Result<Self, ManifestRuntimeError> {
        let mut child = Command::new(program)
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let stdin = child
            .stdin
            .take()
            .ok_or(ManifestRuntimeError::StateUnavailable)?;
        let stdout = child
            .stdout
            .take()
            .ok_or(ManifestRuntimeError::StateUnavailable)?;
        Ok(Self {
            io: Mutex::new(ProcessIo {
                _child: child,
                stdin: BufWriter::new(stdin),
                stdout: BufReader::new(stdout),
            }),
        })
    }

    fn request(&self, request: &CryptoRequest) -> Result<CryptoResponse, ManifestRuntimeError> {
        let mut io = self
            .io
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        serde_json::to_writer(&mut io.stdin, request)
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        io.stdin
            .write_all(b"\n")
            .and_then(|_| io.stdin.flush())
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let mut line = String::new();
        if io
            .stdout
            .read_line(&mut line)
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            == 0
        {
            return Err(ManifestRuntimeError::StateUnavailable);
        }
        let response: CryptoResponse =
            serde_json::from_str(&line).map_err(|_| ManifestRuntimeError::CorruptPersistence)?;
        if response.error.is_some() {
            return Err(ManifestRuntimeError::UnsupportedOperator);
        }
        Ok(response)
    }
}

impl Drop for ProcessIo {
    fn drop(&mut self) {
        let _ = self._child.kill();
        let _ = self._child.wait();
    }
}

impl ManifestCryptoBackend for ProcessManifestBackend {
    fn evaluate(
        &self,
        opcode: &str,
        arguments: &[ManifestValue],
    ) -> Result<ManifestValue, ManifestRuntimeError> {
        let response = self.request(&CryptoRequest::Evaluate {
            opcode: opcode.to_owned(),
            arguments: arguments.iter().map(WireValue::from_manifest).collect(),
        })?;
        response
            .value
            .ok_or(ManifestRuntimeError::CorruptPersistence)?
            .into_manifest()
    }

    fn secret_bool(&self, value: &ManifestValue) -> Result<bool, ManifestRuntimeError> {
        self.request(&CryptoRequest::SecretBool {
            value: WireValue::from_manifest(value),
        })?
        .boolean
        .ok_or(ManifestRuntimeError::CorruptPersistence)
    }

    fn pick(&self, value: &ManifestValue) -> Result<ManifestValue, ManifestRuntimeError> {
        self.request(&CryptoRequest::Pick {
            value: WireValue::from_manifest(value),
        })?
        .value
        .ok_or(ManifestRuntimeError::CorruptPersistence)?
        .into_manifest()
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(2 + bytes.len() * 2);
    output.push_str("0x");
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PlaintextManifestBackend;

    #[test]
    fn wire_value_round_trips_opaque_ciphertext() {
        let value = PlaintextManifestBackend::fhe_uint(42);
        let wire = WireValue::from_manifest(&value);
        let decoded = wire.into_manifest().expect("decode");
        assert_eq!(decoded.value_type(), ValueType::FheUint);
        assert_eq!(decoded.backend_bytes(), value.backend_bytes());
    }
}
