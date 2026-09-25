//! Backend-neutral executor for manifests emitted by `ppsc-compiler`.
//!
//! A committee daemon supplies invocation context and protected inputs, then
//! persists the returned writes and submits their commitments on chain.

use postgres::{Client, NoTls};
use ppsc_compiler::{
    ContractManifest, FunctionManifest, OperatorNode, Representation, StateManifest, ValueType,
};
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::sync::Mutex;

const FHE_UINT_PREFIX: &[u8] = b"PPSC_MANIFEST_DEV_FHE_UINT_V1";
const FHE_BOOL_PREFIX: &[u8] = b"PPSC_MANIFEST_DEV_FHE_BOOL_V1";
const SS_UINT_PREFIX: &[u8] = b"PPSC_MANIFEST_DEV_SS_UINT_V1";
const SS_BOOL_PREFIX: &[u8] = b"PPSC_MANIFEST_DEV_SS_BOOL_V1";
const OPENED_PREFIX: &[u8] = b"PPSC_MANIFEST_DEV_OPENED_V1";

/// A typed runtime value. Protected payloads are opaque to the executor and
/// interpreted only by the selected cryptographic backend.
#[derive(Clone, PartialEq, Eq)]
pub struct ManifestValue {
    value_type: ValueType,
    bytes: Vec<u8>,
}

impl ManifestValue {
    pub fn address(value: [u8; 20]) -> Self {
        Self {
            value_type: ValueType::Address,
            bytes: value.to_vec(),
        }
    }

    pub fn public_uint(value: u128) -> Self {
        Self {
            value_type: ValueType::Uint,
            bytes: value.to_be_bytes().to_vec(),
        }
    }

    pub fn protected(value_type: ValueType, bytes: Vec<u8>) -> Result<Self, ManifestRuntimeError> {
        if !matches!(
            value_type,
            ValueType::FheUint
                | ValueType::FheBool
                | ValueType::Sint
                | ValueType::SecretBool
                | ValueType::Opened
        ) || bytes.is_empty()
        {
            return Err(ManifestRuntimeError::InvalidValue);
        }
        Ok(Self { value_type, bytes })
    }

    pub const fn value_type(&self) -> ValueType {
        self.value_type
    }

    /// Opaque backend serialization. Do not log secret-sharing payloads.
    pub fn backend_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Rebuild a value received from a local cryptographic backend process.
    /// Public values are validated with the same fixed-width rules as values
    /// created by the manifest executor.
    pub fn from_backend_parts(
        value_type: ValueType,
        bytes: Vec<u8>,
    ) -> Result<Self, ManifestRuntimeError> {
        match value_type {
            ValueType::Address if bytes.len() == 20 => Ok(Self { value_type, bytes }),
            ValueType::Uint if bytes.len() == 16 => Ok(Self { value_type, bytes }),
            ValueType::FheUint
            | ValueType::FheBool
            | ValueType::Sint
            | ValueType::SecretBool
            | ValueType::Opened
                if !bytes.is_empty() =>
            {
                Ok(Self { value_type, bytes })
            }
            _ => Err(ManifestRuntimeError::InvalidValue),
        }
    }

    fn state_key(&self) -> Result<Vec<u8>, ManifestRuntimeError> {
        if self.value_type != ValueType::Address || self.bytes.len() != 20 {
            return Err(ManifestRuntimeError::InvalidStateKey);
        }
        Ok(self.bytes.clone())
    }

    fn public_u128(&self) -> Result<u128, ManifestRuntimeError> {
        if self.value_type != ValueType::Uint || self.bytes.len() != 16 {
            return Err(ManifestRuntimeError::InvalidValue);
        }
        let bytes: [u8; 16] = self
            .bytes
            .as_slice()
            .try_into()
            .map_err(|_| ManifestRuntimeError::InvalidValue)?;
        Ok(u128::from_be_bytes(bytes))
    }
}

pub struct InvocationContext {
    pub sender: [u8; 20],
}

/// Inputs use source parameter names. Private inputs contain bytes fetched by
/// the daemon from the chain-recorded dataId locations.
#[derive(Default)]
pub struct InvocationInputs {
    values: BTreeMap<String, ManifestValue>,
}

impl InvocationInputs {
    pub fn insert(&mut self, name: impl Into<String>, value: ManifestValue) {
        self.values.insert(name.into(), value);
    }

    fn get(&self, name: &str) -> Option<&ManifestValue> {
        self.values.get(name)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StateWrite {
    pub state: String,
    pub key: Vec<u8>,
    pub value: ManifestValue,
}

/// Custody calls are plans. The daemon executes them only with an attested
/// successful state transition, so a failed `require` cannot move tokens.
#[derive(Clone, PartialEq, Eq)]
pub struct HostCall {
    pub opcode: String,
    pub arguments: Vec<ManifestValue>,
}

pub struct ExecutionOutcome {
    pub state_writes: Vec<StateWrite>,
    pub host_calls: Vec<HostCall>,
    pub opening: Option<ManifestValue>,
}

pub struct OutboxRecord {
    pub execution_id: [u8; 32],
    pub result_payload: Vec<u8>,
}

pub trait ManifestStateStore: Send + Sync {
    fn load(&self, state: &str, key: &[u8]) -> Result<ManifestValue, ManifestRuntimeError>;

    /// The complete write set must become visible atomically.
    fn commit(&self, writes: &[StateWrite]) -> Result<(), ManifestRuntimeError>;
}

/// Integration boundary for real MPC/FHE engines. Production implementations
/// keep payload bytes opaque and dispatch operations to committee protocols.
pub trait ManifestCryptoBackend: Send + Sync {
    fn evaluate(
        &self,
        opcode: &str,
        arguments: &[ManifestValue],
    ) -> Result<ManifestValue, ManifestRuntimeError>;

    fn secret_bool(&self, value: &ManifestValue) -> Result<bool, ManifestRuntimeError>;

    fn pick(&self, value: &ManifestValue) -> Result<ManifestValue, ManifestRuntimeError>;
}

pub struct ManifestExecutor<'a, S, B> {
    manifest: &'a ContractManifest,
    store: &'a S,
    backend: &'a B,
}

impl<'a, S, B> ManifestExecutor<'a, S, B>
where
    S: ManifestStateStore,
    B: ManifestCryptoBackend,
{
    pub const fn new(manifest: &'a ContractManifest, store: &'a S, backend: &'a B) -> Self {
        Self {
            manifest,
            store,
            backend,
        }
    }

    pub fn execute(
        &self,
        function_name: &str,
        context: &InvocationContext,
        inputs: &InvocationInputs,
    ) -> Result<ExecutionOutcome, ManifestRuntimeError> {
        let outcome = self.evaluate(function_name, context, inputs)?;
        self.store.commit(&outcome.state_writes)?;
        Ok(outcome)
    }

    /// Evaluate without mutating state. Daemons use this to atomically commit
    /// the write set, durable outbox record and queue cursor in one transaction.
    pub fn evaluate(
        &self,
        function_name: &str,
        context: &InvocationContext,
        inputs: &InvocationInputs,
    ) -> Result<ExecutionOutcome, ManifestRuntimeError> {
        let function = self
            .manifest
            .functions
            .iter()
            .find(|candidate| candidate.name == function_name)
            .ok_or(ManifestRuntimeError::FunctionNotFound)?;
        self.validate_inputs(function, inputs)?;

        let mut values = HashMap::new();
        values.insert(
            "context:msg.sender".to_owned(),
            ManifestValue::address(context.sender),
        );
        for input in &function.inputs {
            let value = inputs
                .get(&input.name)
                .ok_or(ManifestRuntimeError::MissingInput)?;
            values.insert(format!("input:{}", input.name), value.clone());
        }

        let mut host_calls = Vec::new();
        for operator in &function.operators {
            let output = self.execute_operator(operator, &values, &mut host_calls)?;
            values.insert(operator.id.clone(), output);
        }

        let mut writes = Vec::with_capacity(function.state_writes.len());
        for write in &function.state_writes {
            let state = self.state(&write.state)?;
            let key = match (&write.key, state.key_type) {
                (Some(reference), Some(ValueType::Address)) => {
                    self.resolve(reference, &values)?.state_key()?
                }
                (None, None) => Vec::new(),
                _ => return Err(ManifestRuntimeError::InvalidStateKey),
            };
            let value = self.resolve(&write.value, &values)?.clone();
            if value.value_type() != state.value_type {
                return Err(ManifestRuntimeError::TypeMismatch);
            }
            writes.push(StateWrite {
                state: write.state.clone(),
                key,
                value,
            });
        }

        let opening = function
            .opening_output
            .as_deref()
            .map(|reference| self.resolve(reference, &values).cloned())
            .transpose()?;
        Ok(ExecutionOutcome {
            state_writes: writes,
            host_calls,
            opening,
        })
    }

    fn execute_operator(
        &self,
        operator: &OperatorNode,
        values: &HashMap<String, ManifestValue>,
        host_calls: &mut Vec<HostCall>,
    ) -> Result<ManifestValue, ManifestRuntimeError> {
        let arguments = operator
            .inputs
            .iter()
            .map(|reference| self.resolve_owned(reference, values))
            .collect::<Result<Vec<_>, _>>()?;
        match operator.opcode.as_str() {
            "load_state" => {
                let state = operator
                    .state
                    .as_deref()
                    .ok_or(ManifestRuntimeError::InvalidManifest)?;
                let declaration = self.state(state)?;
                let key = match declaration.key_type {
                    Some(ValueType::Address) => arguments
                        .first()
                        .ok_or(ManifestRuntimeError::InvalidManifest)?
                        .state_key()?,
                    None if arguments.is_empty() => Vec::new(),
                    _ => return Err(ManifestRuntimeError::InvalidStateKey),
                };
                let value = self.store.load(state, &key)?;
                if value.value_type() != operator.output_type {
                    return Err(ManifestRuntimeError::TypeMismatch);
                }
                Ok(value)
            }
            "require_secret" => {
                let condition = arguments
                    .first()
                    .ok_or(ManifestRuntimeError::InvalidManifest)?;
                if !self.backend.secret_bool(condition)? {
                    return Err(ManifestRuntimeError::PolicyRejected);
                }
                Ok(void_value())
            }
            "opening.pick" => {
                let value = arguments
                    .first()
                    .ok_or(ManifestRuntimeError::InvalidManifest)?;
                let opened = self.backend.pick(value)?;
                if opened.value_type() != ValueType::Opened {
                    return Err(ManifestRuntimeError::TypeMismatch);
                }
                Ok(opened)
            }
            "host.receive_encrypted_token" | "host.send_encrypted_token" => {
                host_calls.push(HostCall {
                    opcode: operator.opcode.clone(),
                    arguments,
                });
                Ok(void_value())
            }
            _ => {
                if !matches!(
                    operator.domain,
                    Representation::Fhe | Representation::SecretSharing
                ) {
                    return Err(ManifestRuntimeError::UnsupportedOperator);
                }
                let value = self.backend.evaluate(&operator.opcode, &arguments)?;
                if value.value_type() != operator.output_type {
                    return Err(ManifestRuntimeError::TypeMismatch);
                }
                Ok(value)
            }
        }
    }

    fn validate_inputs(
        &self,
        function: &FunctionManifest,
        inputs: &InvocationInputs,
    ) -> Result<(), ManifestRuntimeError> {
        if inputs.values.len() != function.inputs.len() {
            return Err(ManifestRuntimeError::MissingInput);
        }
        for declared in &function.inputs {
            let supplied = inputs
                .get(&declared.name)
                .ok_or(ManifestRuntimeError::MissingInput)?;
            if supplied.value_type() != declared.source_type {
                return Err(ManifestRuntimeError::TypeMismatch);
            }
        }
        Ok(())
    }

    fn resolve<'b>(
        &self,
        reference: &str,
        values: &'b HashMap<String, ManifestValue>,
    ) -> Result<&'b ManifestValue, ManifestRuntimeError> {
        values
            .get(reference)
            .ok_or(ManifestRuntimeError::UnknownReference)
    }

    fn resolve_owned(
        &self,
        reference: &str,
        values: &HashMap<String, ManifestValue>,
    ) -> Result<ManifestValue, ManifestRuntimeError> {
        if let Some(constant) = reference.strip_prefix("const:") {
            let value = constant
                .parse::<u128>()
                .map_err(|_| ManifestRuntimeError::InvalidManifest)?;
            return Ok(ManifestValue::public_uint(value));
        }
        self.resolve(reference, values).cloned()
    }

    fn state(&self, name: &str) -> Result<&StateManifest, ManifestRuntimeError> {
        self.manifest
            .state
            .iter()
            .find(|state| state.name == name)
            .ok_or(ManifestRuntimeError::InvalidManifest)
    }
}

#[derive(Default)]
pub struct InMemoryManifestState {
    cells: Mutex<BTreeMap<(String, Vec<u8>), ManifestValue>>,
}

const POSTGRES_MIGRATION: &str = include_str!("../migrations/0002_manifest_runtime.sql");

/// Persistent state owned by one committee node. Give every node a different
/// DATABASE_URL; handoff copies only the selected contract rows to successors.
pub struct PostgresManifestState {
    contract_id: [u8; 32],
    client: Mutex<Client>,
}

pub struct PendingUpload {
    pub data_id: [u8; 32],
    pub owner: [u8; 20],
    pub value: ManifestValue,
    pub commitment: [u8; 32],
}

impl PostgresManifestState {
    pub fn connect(
        database_url: &str,
        contract_id: [u8; 32],
    ) -> Result<Self, ManifestRuntimeError> {
        let mut client = Client::connect(database_url, NoTls).map_err(db_error)?;
        client.batch_execute(POSTGRES_MIGRATION).map_err(db_error)?;
        Ok(Self {
            contract_id,
            client: Mutex::new(client),
        })
    }

    pub fn initialize(
        &self,
        state: &str,
        key: &[u8],
        value: &ManifestValue,
    ) -> Result<(), ManifestRuntimeError> {
        let value_type = value_type_code(value.value_type())?;
        self.client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .execute(
                "INSERT INTO ppsc_manifest_state
                 (contract_id,state_name,state_key,value_type,payload,version)
                 VALUES($1,$2,$3,$4,$5,1)
                 ON CONFLICT(contract_id,state_name,state_key) DO UPDATE SET
                 value_type=EXCLUDED.value_type,payload=EXCLUDED.payload,
                 version=ppsc_manifest_state.version+1,updated_at=now()",
                &[
                    &self.contract_id.as_slice(),
                    &state,
                    &key,
                    &value_type,
                    &value.backend_bytes(),
                ],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn store_input(
        &self,
        data_id: [u8; 32],
        value: &ManifestValue,
    ) -> Result<(), ManifestRuntimeError> {
        let value_type = value_type_code(value.value_type())?;
        self.client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .execute(
                "INSERT INTO ppsc_manifest_inputs(data_id,value_type,payload)
                 VALUES($1,$2,$3) ON CONFLICT(data_id) DO UPDATE SET
                 value_type=EXCLUDED.value_type,payload=EXCLUDED.payload,updated_at=now()",
                &[&data_id.as_slice(), &value_type, &value.backend_bytes()],
            )
            .map_err(db_error)?;
        Ok(())
    }

    /// Atomically stores an authenticated opaque input and creates the durable
    /// chain-registration outbox entry. Inputs are immutable by `data_id`.
    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_upload(
        &self,
        control_address: &str,
        owner: [u8; 20],
        nonce: u64,
        deadline: u64,
        data_id: [u8; 32],
        commitment: [u8; 32],
        value: &ManifestValue,
    ) -> Result<(), ManifestRuntimeError> {
        let value_type = value_type_code(value.value_type())?;
        let nonce = i64::try_from(nonce).map_err(|_| ManifestRuntimeError::InvalidValue)?;
        let deadline = i64::try_from(deadline).map_err(|_| ManifestRuntimeError::InvalidValue)?;
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let mut transaction = client.transaction().map_err(db_error)?;

        let inserted = transaction
            .execute(
                "INSERT INTO ppsc_manifest_inputs(data_id,value_type,payload)
                 VALUES($1,$2,$3) ON CONFLICT(data_id) DO NOTHING",
                &[&data_id.as_slice(), &value_type, &value.backend_bytes()],
            )
            .map_err(db_error)?;
        if inserted == 0 {
            let row = transaction
                .query_one(
                    "SELECT value_type,payload FROM ppsc_manifest_inputs WHERE data_id=$1",
                    &[&data_id.as_slice()],
                )
                .map_err(db_error)?;
            let existing_type: i16 = row.get(0);
            let existing_payload: Vec<u8> = row.get(1);
            if existing_type != value_type || existing_payload != value.backend_bytes() {
                return Err(ManifestRuntimeError::InvalidValue);
            }
        }

        let queued = transaction
            .execute(
                "INSERT INTO ppsc_manifest_uploads
                 (control_address,contract_id,data_id,owner,upload_nonce,deadline,value_type,commitment)
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING",
                &[
                    &control_address,
                    &self.contract_id.as_slice(),
                    &data_id.as_slice(),
                    &owner.as_slice(),
                    &nonce,
                    &deadline,
                    &value_type,
                    &commitment.as_slice(),
                ],
            )
            .map_err(db_error)?;
        if queued == 0 {
            let row = transaction
                .query_opt(
                    "SELECT data_id,value_type,commitment FROM ppsc_manifest_uploads
                     WHERE control_address=$1 AND contract_id=$2 AND owner=$3 AND upload_nonce=$4",
                    &[
                        &control_address,
                        &self.contract_id.as_slice(),
                        &owner.as_slice(),
                        &nonce,
                    ],
                )
                .map_err(db_error)?
                .ok_or(ManifestRuntimeError::InvalidValue)?;
            let existing_id: Vec<u8> = row.get(0);
            let existing_type: i16 = row.get(1);
            let existing_commitment: Vec<u8> = row.get(2);
            if existing_id != data_id
                || existing_type != value_type
                || existing_commitment != commitment
            {
                return Err(ManifestRuntimeError::InvalidValue);
            }
        }
        transaction.commit().map_err(db_error)
    }

    pub fn next_pending_upload(
        &self,
        control_address: &str,
    ) -> Result<Option<PendingUpload>, ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let row = client
            .query_opt(
                "SELECT u.data_id,u.owner,u.value_type,i.payload,u.commitment
                 FROM ppsc_manifest_uploads u
                 JOIN ppsc_manifest_inputs i ON i.data_id=u.data_id
                 WHERE u.control_address=$1 AND u.contract_id=$2 AND u.registered=FALSE
                 ORDER BY u.created_at,u.data_id LIMIT 1",
                &[&control_address, &self.contract_id.as_slice()],
            )
            .map_err(db_error)?;
        row.map(|row| {
            let data_id: Vec<u8> = row.get(0);
            let owner: Vec<u8> = row.get(1);
            let value_type: i16 = row.get(2);
            let payload: Vec<u8> = row.get(3);
            let commitment: Vec<u8> = row.get(4);
            Ok(PendingUpload {
                data_id: data_id
                    .try_into()
                    .map_err(|_| ManifestRuntimeError::CorruptPersistence)?,
                owner: owner
                    .try_into()
                    .map_err(|_| ManifestRuntimeError::CorruptPersistence)?,
                value: value_from_row(value_type, payload)?,
                commitment: commitment
                    .try_into()
                    .map_err(|_| ManifestRuntimeError::CorruptPersistence)?,
            })
        })
        .transpose()
    }

    pub fn mark_upload_registered(
        &self,
        control_address: &str,
        data_id: [u8; 32],
    ) -> Result<(), ManifestRuntimeError> {
        let updated = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .execute(
                "UPDATE ppsc_manifest_uploads SET registered=TRUE,attempts=attempts+1,
                 last_error=NULL,updated_at=now()
                 WHERE control_address=$1 AND contract_id=$2 AND data_id=$3",
                &[
                    &control_address,
                    &self.contract_id.as_slice(),
                    &data_id.as_slice(),
                ],
            )
            .map_err(db_error)?;
        if updated != 1 {
            return Err(ManifestRuntimeError::StateNotFound);
        }
        Ok(())
    }

    pub fn mark_upload_failed(
        &self,
        control_address: &str,
        data_id: [u8; 32],
        error: &str,
    ) -> Result<(), ManifestRuntimeError> {
        self.client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .execute(
                "UPDATE ppsc_manifest_uploads SET attempts=attempts+1,last_error=$4,updated_at=now()
                 WHERE control_address=$1 AND contract_id=$2 AND data_id=$3 AND registered=FALSE",
                &[&control_address, &self.contract_id.as_slice(), &data_id.as_slice(), &error],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub fn input(&self, data_id: [u8; 32]) -> Result<ManifestValue, ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let row = client
            .query_opt(
                "SELECT value_type,payload FROM ppsc_manifest_inputs WHERE data_id=$1",
                &[&data_id.as_slice()],
            )
            .map_err(db_error)?
            .ok_or(ManifestRuntimeError::StateNotFound)?;
        value_from_row(row.get(0), row.get(1))
    }

    pub fn next_execution_index(
        &self,
        control_address: &str,
        node_id: &str,
    ) -> Result<u64, ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        client
            .execute(
                "INSERT INTO ppsc_manifest_daemon_cursors(control_address,node_id)
                 VALUES($1,$2) ON CONFLICT DO NOTHING",
                &[&control_address, &node_id],
            )
            .map_err(db_error)?;
        let row = client
            .query_one(
                "SELECT next_execution_index FROM ppsc_manifest_daemon_cursors
                 WHERE control_address=$1 AND node_id=$2",
                &[&control_address, &node_id],
            )
            .map_err(db_error)?;
        let index: i64 = row.get(0);
        u64::try_from(index).map_err(|_| ManifestRuntimeError::CorruptPersistence)
    }

    /// Atomically records the outcome and advances exactly one queue position.
    pub fn complete_execution(
        &self,
        control_address: &str,
        node_id: &str,
        queue_index: u64,
        execution_id: [u8; 32],
        outcome: TaskOutcome,
    ) -> Result<(), ManifestRuntimeError> {
        let queue_index =
            i64::try_from(queue_index).map_err(|_| ManifestRuntimeError::CorruptPersistence)?;
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let mut transaction = client.transaction().map_err(db_error)?;
        let row = transaction
            .query_opt(
                "SELECT next_execution_index FROM ppsc_manifest_daemon_cursors
                 WHERE control_address=$1 AND node_id=$2 FOR UPDATE",
                &[&control_address, &node_id],
            )
            .map_err(db_error)?
            .ok_or(ManifestRuntimeError::CursorMismatch)?;
        let expected: i64 = row.get(0);
        if expected != queue_index {
            return Err(ManifestRuntimeError::CursorMismatch);
        }
        transaction
            .execute(
                "INSERT INTO ppsc_manifest_processed_tasks
                 (control_address,node_id,queue_index,execution_id,outcome)
                 VALUES($1,$2,$3,$4,$5)",
                &[
                    &control_address,
                    &node_id,
                    &queue_index,
                    &execution_id.as_slice(),
                    &outcome.as_str(),
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE ppsc_manifest_daemon_cursors SET next_execution_index=$3,updated_at=now()
                 WHERE control_address=$1 AND node_id=$2",
                &[&control_address, &node_id, &(queue_index + 1)],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    /// Atomically commits a successful computation, creates the transaction
    /// outbox entry and advances the durable chain queue cursor.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_computation(
        &self,
        control_address: &str,
        node_id: &str,
        queue_index: u64,
        execution_id: [u8; 32],
        writes: &[StateWrite],
        result_payload: &[u8],
    ) -> Result<(), ManifestRuntimeError> {
        let queue_index =
            i64::try_from(queue_index).map_err(|_| ManifestRuntimeError::CorruptPersistence)?;
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let mut transaction = client.transaction().map_err(db_error)?;
        let row = transaction
            .query_opt(
                "SELECT next_execution_index FROM ppsc_manifest_daemon_cursors
                 WHERE control_address=$1 AND node_id=$2 FOR UPDATE",
                &[&control_address, &node_id],
            )
            .map_err(db_error)?
            .ok_or(ManifestRuntimeError::CursorMismatch)?;
        let expected: i64 = row.get(0);
        if expected != queue_index {
            return Err(ManifestRuntimeError::CursorMismatch);
        }
        for write in writes {
            let value_type = value_type_code(write.value.value_type())?;
            transaction
                .execute(
                    "INSERT INTO ppsc_manifest_state
                     (contract_id,state_name,state_key,value_type,payload,version)
                     VALUES($1,$2,$3,$4,$5,1)
                     ON CONFLICT(contract_id,state_name,state_key) DO UPDATE SET
                     value_type=EXCLUDED.value_type,payload=EXCLUDED.payload,
                     version=ppsc_manifest_state.version+1,updated_at=now()",
                    &[
                        &self.contract_id.as_slice(),
                        &write.state,
                        &write.key,
                        &value_type,
                        &write.value.backend_bytes(),
                    ],
                )
                .map_err(db_error)?;
        }
        transaction
            .execute(
                "INSERT INTO ppsc_manifest_result_outbox
                 (control_address,node_id,execution_id,result_payload)
                 VALUES($1,$2,$3,$4)",
                &[
                    &control_address,
                    &node_id,
                    &execution_id.as_slice(),
                    &result_payload,
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "INSERT INTO ppsc_manifest_processed_tasks
                 (control_address,node_id,queue_index,execution_id,outcome)
                 VALUES($1,$2,$3,$4,'completed')",
                &[
                    &control_address,
                    &node_id,
                    &queue_index,
                    &execution_id.as_slice(),
                ],
            )
            .map_err(db_error)?;
        transaction
            .execute(
                "UPDATE ppsc_manifest_daemon_cursors SET next_execution_index=$3,updated_at=now()
                 WHERE control_address=$1 AND node_id=$2",
                &[&control_address, &node_id, &(queue_index + 1)],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub fn pending_outbox_count(
        &self,
        control_address: &str,
        node_id: &str,
    ) -> Result<u64, ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let row = client
            .query_one(
                "SELECT count(*) FROM ppsc_manifest_result_outbox
                 WHERE control_address=$1 AND node_id=$2 AND submitted=FALSE",
                &[&control_address, &node_id],
            )
            .map_err(db_error)?;
        let count: i64 = row.get(0);
        u64::try_from(count).map_err(|_| ManifestRuntimeError::CorruptPersistence)
    }

    pub fn next_pending_outbox(&self) -> Result<Option<OutboxRecord>, ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let row = client
            .query_opt(
                "SELECT execution_id,result_payload FROM ppsc_manifest_result_outbox
                 WHERE submitted=FALSE ORDER BY created_at,execution_id LIMIT 1",
                &[],
            )
            .map_err(db_error)?;
        row.map(|row| {
            let execution_id: Vec<u8> = row.get(0);
            Ok(OutboxRecord {
                execution_id: execution_id
                    .try_into()
                    .map_err(|_| ManifestRuntimeError::CorruptPersistence)?,
                result_payload: row.get(1),
            })
        })
        .transpose()
    }

    pub fn mark_outbox_submitted(
        &self,
        control_address: &str,
        node_id: &str,
        execution_id: [u8; 32],
    ) -> Result<(), ManifestRuntimeError> {
        let updated = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .execute(
                "UPDATE ppsc_manifest_result_outbox SET submitted=TRUE,
                 attempts=attempts+1,last_error=NULL,updated_at=now()
                 WHERE control_address=$1 AND node_id=$2 AND execution_id=$3
                 AND submitted=FALSE",
                &[&control_address, &node_id, &execution_id.as_slice()],
            )
            .map_err(db_error)?;
        if updated != 1 {
            return Err(ManifestRuntimeError::StateNotFound);
        }
        Ok(())
    }
}

impl ManifestStateStore for PostgresManifestState {
    fn load(&self, state: &str, key: &[u8]) -> Result<ManifestValue, ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let row = client
            .query_opt(
                "SELECT value_type,payload FROM ppsc_manifest_state
                 WHERE contract_id=$1 AND state_name=$2 AND state_key=$3",
                &[&self.contract_id.as_slice(), &state, &key],
            )
            .map_err(db_error)?
            .ok_or(ManifestRuntimeError::StateNotFound)?;
        value_from_row(row.get(0), row.get(1))
    }

    fn commit(&self, writes: &[StateWrite]) -> Result<(), ManifestRuntimeError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        let mut transaction = client.transaction().map_err(db_error)?;
        for write in writes {
            let value_type = value_type_code(write.value.value_type())?;
            transaction
                .execute(
                    "INSERT INTO ppsc_manifest_state
                     (contract_id,state_name,state_key,value_type,payload,version)
                     VALUES($1,$2,$3,$4,$5,1)
                     ON CONFLICT(contract_id,state_name,state_key) DO UPDATE SET
                     value_type=EXCLUDED.value_type,payload=EXCLUDED.payload,
                     version=ppsc_manifest_state.version+1,updated_at=now()",
                    &[
                        &self.contract_id.as_slice(),
                        &write.state,
                        &write.key,
                        &value_type,
                        &write.value.backend_bytes(),
                    ],
                )
                .map_err(db_error)?;
        }
        transaction.commit().map_err(db_error)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TaskOutcome {
    Completed,
    Skipped,
    Rejected,
}

impl TaskOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Rejected => "rejected",
        }
    }
}

fn value_type_code(value_type: ValueType) -> Result<i16, ManifestRuntimeError> {
    match value_type {
        ValueType::Address => Ok(0),
        ValueType::Uint => Ok(1),
        ValueType::FheUint => Ok(2),
        ValueType::FheBool => Ok(3),
        ValueType::Sint => Ok(4),
        ValueType::SecretBool => Ok(5),
        ValueType::Opened => Ok(6),
        ValueType::Void => Err(ManifestRuntimeError::InvalidValue),
    }
}

fn value_from_row(code: i16, bytes: Vec<u8>) -> Result<ManifestValue, ManifestRuntimeError> {
    let value_type = match code {
        0 => ValueType::Address,
        1 => ValueType::Uint,
        2 => ValueType::FheUint,
        3 => ValueType::FheBool,
        4 => ValueType::Sint,
        5 => ValueType::SecretBool,
        6 => ValueType::Opened,
        _ => return Err(ManifestRuntimeError::CorruptPersistence),
    };
    if bytes.is_empty() {
        return Err(ManifestRuntimeError::CorruptPersistence);
    }
    Ok(ManifestValue { value_type, bytes })
}

fn db_error(_: postgres::Error) -> ManifestRuntimeError {
    ManifestRuntimeError::PersistenceFailure
}

impl InMemoryManifestState {
    pub fn initialize(
        &self,
        state: impl Into<String>,
        key: Vec<u8>,
        value: ManifestValue,
    ) -> Result<(), ManifestRuntimeError> {
        self.cells
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .insert((state.into(), key), value);
        Ok(())
    }
}

impl ManifestStateStore for InMemoryManifestState {
    fn load(&self, state: &str, key: &[u8]) -> Result<ManifestValue, ManifestRuntimeError> {
        self.cells
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?
            .get(&(state.to_owned(), key.to_vec()))
            .cloned()
            .ok_or(ManifestRuntimeError::StateNotFound)
    }

    fn commit(&self, writes: &[StateWrite]) -> Result<(), ManifestRuntimeError> {
        let mut cells = self
            .cells
            .lock()
            .map_err(|_| ManifestRuntimeError::StateUnavailable)?;
        for write in writes {
            cells.insert(
                (write.state.clone(), write.key.clone()),
                write.value.clone(),
            );
        }
        Ok(())
    }
}

/// Development backend. Plaintext lives inside tagged bytes; never use this
/// backend for a public or production deployment.
#[derive(Default)]
pub struct PlaintextManifestBackend;

impl PlaintextManifestBackend {
    pub fn fhe_uint(value: u128) -> ManifestValue {
        encoded(ValueType::FheUint, FHE_UINT_PREFIX, &value.to_be_bytes())
    }

    pub fn secret_uint(value: u128) -> ManifestValue {
        encoded(ValueType::Sint, SS_UINT_PREFIX, &value.to_be_bytes())
    }

    pub fn open_for_tests(value: &ManifestValue) -> Result<u128, ManifestRuntimeError> {
        match value.value_type() {
            ValueType::FheUint => decode_u128(value, FHE_UINT_PREFIX),
            ValueType::Sint => decode_u128(value, SS_UINT_PREFIX),
            ValueType::Opened => decode_u128(value, OPENED_PREFIX),
            ValueType::Uint => value.public_u128(),
            _ => Err(ManifestRuntimeError::TypeMismatch),
        }
    }
}

impl ManifestCryptoBackend for PlaintextManifestBackend {
    fn evaluate(
        &self,
        opcode: &str,
        arguments: &[ManifestValue],
    ) -> Result<ManifestValue, ManifestRuntimeError> {
        match opcode {
            "fhe.encrypt" => Ok(Self::fhe_uint(one(arguments)?.public_u128()?)),
            "fhe.add" => {
                let (left, right) = two_u128(arguments, FHE_UINT_PREFIX)?;
                Ok(Self::fhe_uint(
                    left.checked_add(right)
                        .ok_or(ManifestRuntimeError::Overflow)?,
                ))
            }
            "fhe.sub" => {
                let (left, right) = two_u128(arguments, FHE_UINT_PREFIX)?;
                Ok(Self::fhe_uint(
                    left.checked_sub(right)
                        .ok_or(ManifestRuntimeError::Underflow)?,
                ))
            }
            "fhe.ge" => {
                let (left, right) = two_u128(arguments, FHE_UINT_PREFIX)?;
                Ok(encoded(
                    ValueType::FheBool,
                    FHE_BOOL_PREFIX,
                    &[u8::from(left >= right)],
                ))
            }
            "mpc.add" => {
                let (left, right) = two_u128(arguments, SS_UINT_PREFIX)?;
                Ok(Self::secret_uint(
                    left.checked_add(right)
                        .ok_or(ManifestRuntimeError::Overflow)?,
                ))
            }
            "mpc.sub" => {
                let (left, right) = two_u128(arguments, SS_UINT_PREFIX)?;
                Ok(Self::secret_uint(
                    left.checked_sub(right)
                        .ok_or(ManifestRuntimeError::Underflow)?,
                ))
            }
            "mpc.ge" => {
                let (left, right) = two_u128(arguments, SS_UINT_PREFIX)?;
                Ok(encoded(
                    ValueType::SecretBool,
                    SS_BOOL_PREFIX,
                    &[u8::from(left >= right)],
                ))
            }
            "convert.h2s" => {
                let value = one(arguments)?;
                match value.value_type() {
                    ValueType::FheUint => {
                        Ok(Self::secret_uint(decode_u128(value, FHE_UINT_PREFIX)?))
                    }
                    ValueType::FheBool => Ok(encoded(
                        ValueType::SecretBool,
                        SS_BOOL_PREFIX,
                        &[u8::from(decode_bool(value, FHE_BOOL_PREFIX)?)],
                    )),
                    _ => Err(ManifestRuntimeError::TypeMismatch),
                }
            }
            "convert.s2h" => Ok(Self::fhe_uint(decode_u128(
                one(arguments)?,
                SS_UINT_PREFIX,
            )?)),
            _ => Err(ManifestRuntimeError::UnsupportedOperator),
        }
    }

    fn secret_bool(&self, value: &ManifestValue) -> Result<bool, ManifestRuntimeError> {
        decode_bool(value, SS_BOOL_PREFIX)
    }

    fn pick(&self, value: &ManifestValue) -> Result<ManifestValue, ManifestRuntimeError> {
        let opened = decode_u128(value, SS_UINT_PREFIX)?;
        Ok(encoded(
            ValueType::Opened,
            OPENED_PREFIX,
            &opened.to_be_bytes(),
        ))
    }
}

fn void_value() -> ManifestValue {
    ManifestValue {
        value_type: ValueType::Void,
        bytes: Vec::new(),
    }
}

fn encoded(value_type: ValueType, prefix: &[u8], payload: &[u8]) -> ManifestValue {
    let mut bytes = Vec::with_capacity(prefix.len() + payload.len());
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(payload);
    ManifestValue { value_type, bytes }
}

fn one(arguments: &[ManifestValue]) -> Result<&ManifestValue, ManifestRuntimeError> {
    if arguments.len() != 1 {
        return Err(ManifestRuntimeError::InvalidManifest);
    }
    Ok(&arguments[0])
}

fn two_u128(
    arguments: &[ManifestValue],
    prefix: &[u8],
) -> Result<(u128, u128), ManifestRuntimeError> {
    if arguments.len() != 2 {
        return Err(ManifestRuntimeError::InvalidManifest);
    }
    Ok((
        decode_u128(&arguments[0], prefix)?,
        decode_u128(&arguments[1], prefix)?,
    ))
}

fn decode_u128(value: &ManifestValue, prefix: &[u8]) -> Result<u128, ManifestRuntimeError> {
    if value.bytes.len() != prefix.len() + 16 || !value.bytes.starts_with(prefix) {
        return Err(ManifestRuntimeError::InvalidValue);
    }
    let bytes: [u8; 16] = value.bytes[prefix.len()..]
        .try_into()
        .map_err(|_| ManifestRuntimeError::InvalidValue)?;
    Ok(u128::from_be_bytes(bytes))
}

fn decode_bool(value: &ManifestValue, prefix: &[u8]) -> Result<bool, ManifestRuntimeError> {
    if value.bytes.len() != prefix.len() + 1 || !value.bytes.starts_with(prefix) {
        return Err(ManifestRuntimeError::InvalidValue);
    }
    match value.bytes[prefix.len()] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ManifestRuntimeError::InvalidValue),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestRuntimeError {
    FunctionNotFound,
    MissingInput,
    TypeMismatch,
    UnknownReference,
    InvalidManifest,
    InvalidStateKey,
    InvalidValue,
    StateNotFound,
    StateUnavailable,
    UnsupportedOperator,
    PolicyRejected,
    Overflow,
    Underflow,
    PersistenceFailure,
    CorruptPersistence,
    CursorMismatch,
}

impl fmt::Display for ManifestRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "manifest runtime failed: {self:?}")
    }
}

impl Error for ManifestRuntimeError {}

pub fn manifest_from_json(json: &str) -> Result<ContractManifest, ManifestRuntimeError> {
    serde_json::from_str(json).map_err(|_| ManifestRuntimeError::InvalidManifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ppsc_compiler::compile;

    const CONTRACT: &str = r#"
        privacy contract ConfidentialToken {
          private FheUint balance[address];
          function transfer(address to, FheUint amount) public {
            SecretBool sufficient = H2S(FHE.ge(balance[msg.sender], amount));
            require(sufficient);
            balance[msg.sender] = FHE.sub(balance[msg.sender], amount);
            balance[to] = FHE.add(balance[to], amount);
          }
          function getBalance() public {
            Sint balanceShare = H2S(balance[msg.sender]);
            return Pick(balanceShare);
          }
        }
    "#;

    fn address(byte: u8) -> [u8; 20] {
        [byte; 20]
    }

    #[test]
    fn executes_transfer_and_opening_from_compiler_manifest() {
        let manifest = compile(CONTRACT).expect("compile");
        let state = InMemoryManifestState::default();
        state
            .initialize(
                "balance",
                address(1).to_vec(),
                PlaintextManifestBackend::fhe_uint(100),
            )
            .expect("alice balance");
        state
            .initialize(
                "balance",
                address(2).to_vec(),
                PlaintextManifestBackend::fhe_uint(20),
            )
            .expect("bob balance");
        let backend = PlaintextManifestBackend;
        let executor = ManifestExecutor::new(&manifest, &state, &backend);
        let mut inputs = InvocationInputs::default();
        inputs.insert("to", ManifestValue::address(address(2)));
        inputs.insert("amount", PlaintextManifestBackend::fhe_uint(30));
        let outcome = executor
            .execute(
                "transfer",
                &InvocationContext { sender: address(1) },
                &inputs,
            )
            .expect("transfer");
        assert_eq!(outcome.state_writes.len(), 2);

        let alice = state.load("balance", &address(1)).expect("alice");
        let bob = state.load("balance", &address(2)).expect("bob");
        assert_eq!(PlaintextManifestBackend::open_for_tests(&alice), Ok(70));
        assert_eq!(PlaintextManifestBackend::open_for_tests(&bob), Ok(50));

        let query = executor
            .execute(
                "getBalance",
                &InvocationContext { sender: address(1) },
                &InvocationInputs::default(),
            )
            .expect("opening");
        assert_eq!(
            PlaintextManifestBackend::open_for_tests(
                query.opening.as_ref().expect("opening output")
            ),
            Ok(70)
        );
    }

    #[test]
    fn failed_require_does_not_commit_partial_writes() {
        let manifest = compile(CONTRACT).expect("compile");
        let state = InMemoryManifestState::default();
        state
            .initialize(
                "balance",
                address(1).to_vec(),
                PlaintextManifestBackend::fhe_uint(10),
            )
            .expect("alice balance");
        state
            .initialize(
                "balance",
                address(2).to_vec(),
                PlaintextManifestBackend::fhe_uint(20),
            )
            .expect("bob balance");
        let backend = PlaintextManifestBackend;
        let executor = ManifestExecutor::new(&manifest, &state, &backend);
        let mut inputs = InvocationInputs::default();
        inputs.insert("to", ManifestValue::address(address(2)));
        inputs.insert("amount", PlaintextManifestBackend::fhe_uint(30));
        let result = executor.execute(
            "transfer",
            &InvocationContext { sender: address(1) },
            &inputs,
        );
        assert!(matches!(result, Err(ManifestRuntimeError::PolicyRejected)));
        let alice = state.load("balance", &address(1)).expect("alice");
        let bob = state.load("balance", &address(2)).expect("bob");
        assert_eq!(PlaintextManifestBackend::open_for_tests(&alice), Ok(10));
        assert_eq!(PlaintextManifestBackend::open_for_tests(&bob), Ok(20));
    }

    #[test]
    fn deposit_produces_deferred_host_call() {
        let manifest = compile(
            r#"
            privacy contract Deposit {
              private FheUint balance[address];
              function deposit(FheUint amount) public {
                receiveEncryptedToken(amount);
                balance[msg.sender] = FHE.add(balance[msg.sender], amount);
              }
            }
            "#,
        )
        .expect("compile");
        let state = InMemoryManifestState::default();
        state
            .initialize(
                "balance",
                address(1).to_vec(),
                PlaintextManifestBackend::fhe_uint(0),
            )
            .expect("balance");
        let backend = PlaintextManifestBackend;
        let executor = ManifestExecutor::new(&manifest, &state, &backend);
        let mut inputs = InvocationInputs::default();
        inputs.insert("amount", PlaintextManifestBackend::fhe_uint(25));
        let outcome = executor
            .execute(
                "deposit",
                &InvocationContext { sender: address(1) },
                &inputs,
            )
            .expect("deposit");
        assert_eq!(outcome.host_calls.len(), 1);
        assert_eq!(outcome.host_calls[0].opcode, "host.receive_encrypted_token");
        let balance = state.load("balance", &address(1)).expect("balance");
        assert_eq!(PlaintextManifestBackend::open_for_tests(&balance), Ok(25));
    }

    #[test]
    fn create_account_materializes_public_constant() {
        let manifest = compile(
            r#"
            privacy contract Account {
              private FheUint balance[address];
              function createAccount() public {
                balance[msg.sender] = FHE.encrypt(0);
              }
            }
            "#,
        )
        .expect("compile");
        let state = InMemoryManifestState::default();
        let backend = PlaintextManifestBackend;
        ManifestExecutor::new(&manifest, &state, &backend)
            .execute(
                "createAccount",
                &InvocationContext { sender: address(3) },
                &InvocationInputs::default(),
            )
            .expect("create account");
        let balance = state.load("balance", &address(3)).expect("balance");
        assert_eq!(PlaintextManifestBackend::open_for_tests(&balance), Ok(0));
    }
}
