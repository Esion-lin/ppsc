//! Long-running manifest committee daemon state machine.

use crate::manifest::{
    ExecutionOutcome, InvocationContext, InvocationInputs, ManifestCryptoBackend, ManifestExecutor,
    ManifestRuntimeError, ManifestValue, PlaintextManifestBackend, PostgresManifestState,
    TaskOutcome,
};
use ppsc_compiler::{ContractManifest, FunctionManifest, ValueType};
use std::error::Error;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChainExecutionStatus {
    None,
    Requested,
    CommitteeAssigned,
    Running,
    Handoff,
    ResultPending,
    Completed,
    Failed,
    Cancelled,
}

impl ChainExecutionStatus {
    pub fn from_u8(value: u8) -> Result<Self, DaemonError> {
        match value {
            0 => Ok(Self::None),
            1 => Ok(Self::Requested),
            2 => Ok(Self::CommitteeAssigned),
            3 => Ok(Self::Running),
            4 => Ok(Self::Handoff),
            5 => Ok(Self::ResultPending),
            6 => Ok(Self::Completed),
            7 => Ok(Self::Failed),
            8 => Ok(Self::Cancelled),
            _ => Err(DaemonError::InvalidChainResponse),
        }
    }

    fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

pub struct ManifestChainTask {
    pub execution_id: [u8; 32],
    pub contract_id: [u8; 32],
    pub selector: [u8; 4],
    pub requester: [u8; 20],
    pub private_input_ids: Vec<[u8; 32]>,
    pub public_inputs: Vec<u8>,
    pub status: ChainExecutionStatus,
    pub node_is_current_member: bool,
}

pub struct DecodedOutbox {
    pub execution_id: [u8; 32],
    pub state_writes: Vec<DecodedStateWrite>,
    pub opening: Option<ManifestValue>,
}

pub struct DecodedStateWrite {
    pub state: String,
    pub key: Vec<u8>,
    pub value: ManifestValue,
}

pub trait ManifestTaskSource: Send + Sync {
    fn execution_count(&self) -> Result<u64, DaemonError>;
    fn task_at(&self, index: u64) -> Result<ManifestChainTask, DaemonError>;
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    Idle,
    Waiting,
    Skipped { index: u64, execution_id: [u8; 32] },
    Computed { index: u64, execution_id: [u8; 32] },
}

pub struct ManifestCommitteeDaemon<'a, C, B> {
    chain: &'a C,
    store: &'a PostgresManifestState,
    backend: &'a B,
    manifest: &'a ContractManifest,
    contract_id: [u8; 32],
    control_address: &'a str,
    node_id: &'a str,
}

impl<'a, C, B> ManifestCommitteeDaemon<'a, C, B>
where
    C: ManifestTaskSource,
    B: ManifestCryptoBackend,
{
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        chain: &'a C,
        store: &'a PostgresManifestState,
        backend: &'a B,
        manifest: &'a ContractManifest,
        contract_id: [u8; 32],
        control_address: &'a str,
        node_id: &'a str,
    ) -> Self {
        Self {
            chain,
            store,
            backend,
            manifest,
            contract_id,
            control_address,
            node_id,
        }
    }

    pub fn poll_once(&self) -> Result<PollOutcome, DaemonError> {
        let index = self
            .store
            .next_execution_index(self.control_address, self.node_id)?;
        if index >= self.chain.execution_count()? {
            return Ok(PollOutcome::Idle);
        }
        let task = self.chain.task_at(index)?;
        if task.contract_id != self.contract_id {
            self.store.complete_execution(
                self.control_address,
                self.node_id,
                index,
                task.execution_id,
                TaskOutcome::Skipped,
            )?;
            return Ok(PollOutcome::Skipped {
                index,
                execution_id: task.execution_id,
            });
        }
        if task.status.terminal() {
            self.store.complete_execution(
                self.control_address,
                self.node_id,
                index,
                task.execution_id,
                TaskOutcome::Skipped,
            )?;
            return Ok(PollOutcome::Skipped {
                index,
                execution_id: task.execution_id,
            });
        }
        // A node must not advance past a live task merely because it is not in
        // the current committee: a later handoff may select this node.
        if task.status != ChainExecutionStatus::Running || !task.node_is_current_member {
            return Ok(PollOutcome::Waiting);
        }

        let function = function_by_selector(self.manifest, task.selector)?;
        let inputs = invocation_inputs(function, &task, self.store)?;
        let outcome = ManifestExecutor::new(self.manifest, self.store, self.backend).evaluate(
            &function.name,
            &InvocationContext {
                sender: task.requester,
            },
            &inputs,
        )?;
        let payload = encode_outbox_payload(task.execution_id, &outcome)?;
        self.store.commit_computation(
            self.control_address,
            self.node_id,
            index,
            task.execution_id,
            &outcome.state_writes,
            &payload,
        )?;
        Ok(PollOutcome::Computed {
            index,
            execution_id: task.execution_id,
        })
    }
}

fn function_by_selector(
    manifest: &ContractManifest,
    selector: [u8; 4],
) -> Result<&FunctionManifest, DaemonError> {
    manifest
        .functions
        .iter()
        .find(|function| decode_selector(&function.selector) == Ok(selector))
        .ok_or(DaemonError::FunctionNotFound)
}

fn invocation_inputs(
    function: &FunctionManifest,
    task: &ManifestChainTask,
    store: &PostgresManifestState,
) -> Result<InvocationInputs, DaemonError> {
    let public_count = function
        .inputs
        .iter()
        .filter(|input| matches!(input.source_type, ValueType::Address | ValueType::Uint))
        .count();
    if task.public_inputs.len() != public_count * 32 {
        return Err(DaemonError::InvalidPublicInputs);
    }
    let private_count = function.inputs.len() - public_count;
    if task.private_input_ids.len() != private_count {
        return Err(DaemonError::InvalidPrivateInputs);
    }

    let mut public_index = 0;
    let mut private_index = 0;
    let mut inputs = InvocationInputs::default();
    for input in &function.inputs {
        let value = match input.source_type {
            ValueType::Address => {
                let word = &task.public_inputs[public_index * 32..(public_index + 1) * 32];
                if word[..12].iter().any(|byte| *byte != 0) {
                    return Err(DaemonError::InvalidPublicInputs);
                }
                let address = word[12..]
                    .try_into()
                    .map_err(|_| DaemonError::InvalidPublicInputs)?;
                public_index += 1;
                ManifestValue::address(address)
            }
            ValueType::Uint => {
                let word = &task.public_inputs[public_index * 32..(public_index + 1) * 32];
                if word[..16].iter().any(|byte| *byte != 0) {
                    return Err(DaemonError::InvalidPublicInputs);
                }
                let bytes: [u8; 16] = word[16..]
                    .try_into()
                    .map_err(|_| DaemonError::InvalidPublicInputs)?;
                public_index += 1;
                ManifestValue::public_uint(u128::from_be_bytes(bytes))
            }
            ValueType::FheUint | ValueType::Sint => {
                let data_id = task.private_input_ids[private_index];
                private_index += 1;
                let value = store.input(data_id)?;
                if value.value_type() != input.source_type {
                    return Err(DaemonError::InvalidPrivateInputs);
                }
                value
            }
            _ => return Err(DaemonError::UnsupportedParameter),
        };
        inputs.insert(&input.name, value);
    }
    Ok(inputs)
}

fn encode_outbox_payload(
    execution_id: [u8; 32],
    outcome: &ExecutionOutcome,
) -> Result<Vec<u8>, DaemonError> {
    let mut payload = b"PPSC_MANIFEST_OUTBOX_V1".to_vec();
    payload.extend_from_slice(&execution_id);
    append_len(&mut payload, outcome.state_writes.len())?;
    for write in &outcome.state_writes {
        append_bytes(&mut payload, write.state.as_bytes())?;
        append_bytes(&mut payload, &write.key)?;
        payload.push(value_type_tag(write.value.value_type())?);
        append_bytes(&mut payload, write.value.backend_bytes())?;
    }
    append_len(&mut payload, outcome.host_calls.len())?;
    for call in &outcome.host_calls {
        append_bytes(&mut payload, call.opcode.as_bytes())?;
    }
    if let Some(opening) = &outcome.opening {
        payload.push(1);
        payload.push(value_type_tag(opening.value_type())?);
        append_bytes(&mut payload, opening.backend_bytes())?;
    } else {
        payload.push(0);
    }
    Ok(payload)
}

pub fn decode_outbox_payload(payload: &[u8]) -> Result<DecodedOutbox, DaemonError> {
    let prefix = b"PPSC_MANIFEST_OUTBOX_V1";
    if !payload.starts_with(prefix) {
        return Err(DaemonError::InvalidOutput);
    }
    let mut cursor = prefix.len();
    let execution_id = take(payload, &mut cursor, 32)?
        .try_into()
        .map_err(|_| DaemonError::InvalidOutput)?;
    let write_count = take_u32(payload, &mut cursor)? as usize;
    let mut state_writes = Vec::with_capacity(write_count);
    for _ in 0..write_count {
        let state = String::from_utf8(take_sized(payload, &mut cursor)?.to_vec())
            .map_err(|_| DaemonError::InvalidOutput)?;
        let key = take_sized(payload, &mut cursor)?.to_vec();
        let value_type = value_type_from_tag(
            *take(payload, &mut cursor, 1)?
                .first()
                .ok_or(DaemonError::InvalidOutput)?,
        )?;
        let bytes = take_sized(payload, &mut cursor)?.to_vec();
        state_writes.push(DecodedStateWrite {
            state,
            key,
            value: ManifestValue::protected(value_type, bytes)?,
        });
    }
    let host_count = take_u32(payload, &mut cursor)? as usize;
    for _ in 0..host_count {
        let _ = take_sized(payload, &mut cursor)?;
    }
    let has_opening = *take(payload, &mut cursor, 1)?
        .first()
        .ok_or(DaemonError::InvalidOutput)?;
    let opening = match has_opening {
        0 => None,
        1 => {
            let tag = *take(payload, &mut cursor, 1)?
                .first()
                .ok_or(DaemonError::InvalidOutput)?;
            Some(ManifestValue::protected(
                value_type_from_tag(tag)?,
                take_sized(payload, &mut cursor)?.to_vec(),
            )?)
        }
        _ => return Err(DaemonError::InvalidOutput),
    };
    if cursor != payload.len() {
        return Err(DaemonError::InvalidOutput);
    }
    Ok(DecodedOutbox {
        execution_id,
        state_writes,
        opening,
    })
}

fn take<'a>(payload: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8], DaemonError> {
    let end = cursor
        .checked_add(length)
        .ok_or(DaemonError::InvalidOutput)?;
    let value = payload
        .get(*cursor..end)
        .ok_or(DaemonError::InvalidOutput)?;
    *cursor = end;
    Ok(value)
}

fn take_u32(payload: &[u8], cursor: &mut usize) -> Result<u32, DaemonError> {
    let bytes: [u8; 4] = take(payload, cursor, 4)?
        .try_into()
        .map_err(|_| DaemonError::InvalidOutput)?;
    Ok(u32::from_be_bytes(bytes))
}

fn take_sized<'a>(payload: &'a [u8], cursor: &mut usize) -> Result<&'a [u8], DaemonError> {
    let length = take_u32(payload, cursor)? as usize;
    take(payload, cursor, length)
}

fn append_len(output: &mut Vec<u8>, value: usize) -> Result<(), DaemonError> {
    let value = u32::try_from(value).map_err(|_| DaemonError::OutputTooLarge)?;
    output.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

fn append_bytes(output: &mut Vec<u8>, value: &[u8]) -> Result<(), DaemonError> {
    append_len(output, value.len())?;
    output.extend_from_slice(value);
    Ok(())
}

fn value_type_tag(value_type: ValueType) -> Result<u8, DaemonError> {
    match value_type {
        ValueType::Address => Ok(0),
        ValueType::Uint => Ok(1),
        ValueType::FheUint => Ok(2),
        ValueType::FheBool => Ok(3),
        ValueType::Sint => Ok(4),
        ValueType::SecretBool => Ok(5),
        ValueType::Opened => Ok(6),
        ValueType::Void => Err(DaemonError::InvalidOutput),
    }
}

fn value_type_from_tag(tag: u8) -> Result<ValueType, DaemonError> {
    match tag {
        0 => Ok(ValueType::Address),
        1 => Ok(ValueType::Uint),
        2 => Ok(ValueType::FheUint),
        3 => Ok(ValueType::FheBool),
        4 => Ok(ValueType::Sint),
        5 => Ok(ValueType::SecretBool),
        6 => Ok(ValueType::Opened),
        _ => Err(DaemonError::InvalidOutput),
    }
}

pub fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], DaemonError> {
    let value = value.trim();
    let raw = value
        .strip_prefix("0x")
        .ok_or(DaemonError::InvalidChainResponse)?;
    if raw.len() != N * 2 {
        return Err(DaemonError::InvalidChainResponse);
    }
    let mut output = [0_u8; N];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&raw[index * 2..index * 2 + 2], 16)
            .map_err(|_| DaemonError::InvalidChainResponse)?;
    }
    Ok(output)
}

pub fn decode_hex_bytes(value: &str) -> Result<Vec<u8>, DaemonError> {
    let value = value.trim();
    let raw = value
        .strip_prefix("0x")
        .ok_or(DaemonError::InvalidChainResponse)?;
    if raw.len() % 2 != 0 {
        return Err(DaemonError::InvalidChainResponse);
    }
    (0..raw.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&raw[index..index + 2], 16)
                .map_err(|_| DaemonError::InvalidChainResponse)
        })
        .collect()
}

fn decode_selector(value: &str) -> Result<[u8; 4], DaemonError> {
    decode_hex(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonError {
    ChainUnavailable,
    InvalidChainResponse,
    FunctionNotFound,
    InvalidPublicInputs,
    InvalidPrivateInputs,
    UnsupportedParameter,
    InvalidOutput,
    OutputTooLarge,
    Runtime(ManifestRuntimeError),
}

impl From<ManifestRuntimeError> for DaemonError {
    fn from(value: ManifestRuntimeError) -> Self {
        Self::Runtime(value)
    }
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manifest daemon failed: {self:?}")
    }
}

impl Error for DaemonError {}

/// The executable currently uses this backend for local development only.
pub type DevelopmentManifestDaemon<'a, C> =
    ManifestCommitteeDaemon<'a, C, PlaintextManifestBackend>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_execution_status_and_hex() {
        assert!(matches!(
            ChainExecutionStatus::from_u8(3),
            Ok(ChainExecutionStatus::Running)
        ));
        assert_eq!(decode_hex::<4>("0x7d32e7bd"), Ok([0x7d, 0x32, 0xe7, 0xbd]));
        assert!(ChainExecutionStatus::from_u8(99).is_err());
    }

    #[test]
    fn selector_lookup_uses_compiler_artifact() {
        let manifest = ppsc_compiler::compile(
            "privacy contract T { function f(address to) public { return Pick(H2S(FHE.encrypt(0))); } }",
        )
        .expect("compile");
        let selector = decode_selector(&manifest.functions[0].selector).expect("selector");
        assert_eq!(
            function_by_selector(&manifest, selector).map(|function| function.name.as_str()),
            Ok("f")
        );
    }

    #[test]
    fn outbox_payload_round_trips_state_and_opening() {
        let outcome = ExecutionOutcome {
            state_writes: vec![crate::manifest::StateWrite {
                state: "balance".to_owned(),
                key: vec![7; 20],
                value: PlaintextManifestBackend::fhe_uint(42),
            }],
            host_calls: Vec::new(),
            opening: Some(
                PlaintextManifestBackend
                    .pick(&PlaintextManifestBackend::secret_uint(42))
                    .expect("pick"),
            ),
        };
        let encoded = encode_outbox_payload([9; 32], &outcome).expect("encode");
        let decoded = decode_outbox_payload(&encoded).expect("decode");
        assert_eq!(decoded.execution_id, [9; 32]);
        assert_eq!(decoded.state_writes.len(), 1);
        assert_eq!(decoded.state_writes[0].state, "balance");
        assert_eq!(decoded.state_writes[0].key, vec![7; 20]);
        assert_eq!(
            PlaintextManifestBackend::open_for_tests(&decoded.state_writes[0].value),
            Ok(42)
        );
        assert_eq!(
            PlaintextManifestBackend::open_for_tests(decoded.opening.as_ref().expect("opening")),
            Ok(42)
        );
    }
}
