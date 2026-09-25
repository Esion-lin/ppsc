use ppsc_compiler::{ContractManifest, ValueType};
use ppsc_runtime::manifest::{
    manifest_from_json, ManifestCryptoBackend, ManifestRuntimeError, ManifestValue,
    PlaintextManifestBackend, PostgresManifestState,
};
use ppsc_runtime::manifest_crypto_process::ProcessManifestBackend;
use ppsc_runtime::manifest_daemon::{
    decode_hex, decode_hex_bytes, decode_outbox_payload, ChainExecutionStatus, DaemonError,
    ManifestChainTask, ManifestCommitteeDaemon, ManifestTaskSource, PollOutcome,
};
use ppsc_runtime::manifest_upload::{
    decode_fixed, hex as upload_hex, read_http_upload, write_http_json, UploadResponse,
};
use serde_json::json;
use std::env;
use std::error::Error;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tiny_keccak::{Hasher, Keccak};

struct Config {
    database_url: String,
    rpc_url: String,
    control: String,
    gateway: String,
    manifest_path: String,
    contract_id: [u8; 32],
    node_address: String,
    node_id: String,
    node_tx_key: String,
    active_committee_id: String,
    poll_interval: Duration,
    crypto_backend: String,
    crypto_command: Option<String>,
    crypto_arguments: Vec<String>,
    upload_listen: Option<String>,
}

impl Config {
    fn load() -> Result<Self, Box<dyn Error>> {
        let node_address = required("NODE_ADDRESS")?;
        decode_hex::<20>(&node_address)?;
        let node_tx_key = required("NODE_TX_KEY")?;
        let derived = run_cast(&[
            "wallet".to_owned(),
            "address".to_owned(),
            "--private-key".to_owned(),
            node_tx_key.clone(),
        ])?;
        if !derived.eq_ignore_ascii_case(&node_address) {
            return Err("NODE_TX_KEY does not match NODE_ADDRESS".into());
        }
        Ok(Self {
            database_url: required("DATABASE_URL")?,
            rpc_url: required("RPC_URL")?,
            control: required("CONTROL")?,
            gateway: required("GATEWAY")?,
            manifest_path: required("MANIFEST_PATH")?,
            contract_id: decode_hex(&required("CONTRACT_ID")?)?,
            node_id: env::var("NODE_ID").unwrap_or_else(|_| node_address.clone()),
            node_address,
            node_tx_key,
            active_committee_id: required("ACTIVE_COMMITTEE_ID")?,
            poll_interval: Duration::from_secs(
                env::var("POLL_INTERVAL_SECONDS")
                    .unwrap_or_else(|_| "2".to_owned())
                    .parse()?,
            ),
            crypto_backend: env::var("MANIFEST_CRYPTO_BACKEND")
                .unwrap_or_else(|_| "plaintext".to_owned()),
            crypto_command: env::var("MANIFEST_CRYPTO_COMMAND").ok(),
            crypto_arguments: env::var("MANIFEST_CRYPTO_ARGS")
                .unwrap_or_default()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
            upload_listen: env::var("UPLOAD_LISTEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        })
    }
}

enum DaemonCryptoBackend {
    Plaintext(PlaintextManifestBackend),
    Process(ProcessManifestBackend),
}

impl DaemonCryptoBackend {
    fn load(config: &Config) -> Result<Self, Box<dyn Error>> {
        match config.crypto_backend.as_str() {
            "plaintext" => Ok(Self::Plaintext(PlaintextManifestBackend)),
            "process" => {
                let command = config.crypto_command.as_deref().ok_or(
                    "MANIFEST_CRYPTO_COMMAND is required when MANIFEST_CRYPTO_BACKEND=process",
                )?;
                Ok(Self::Process(ProcessManifestBackend::spawn(
                    command,
                    &config.crypto_arguments,
                )?))
            }
            other => Err(format!(
                "unsupported MANIFEST_CRYPTO_BACKEND={other}; expected plaintext or process"
            )
            .into()),
        }
    }
}

impl ManifestCryptoBackend for DaemonCryptoBackend {
    fn evaluate(
        &self,
        opcode: &str,
        arguments: &[ManifestValue],
    ) -> Result<ManifestValue, ManifestRuntimeError> {
        match self {
            Self::Plaintext(backend) => backend.evaluate(opcode, arguments),
            Self::Process(backend) => backend.evaluate(opcode, arguments),
        }
    }

    fn secret_bool(&self, value: &ManifestValue) -> Result<bool, ManifestRuntimeError> {
        match self {
            Self::Plaintext(backend) => backend.secret_bool(value),
            Self::Process(backend) => backend.secret_bool(value),
        }
    }

    fn pick(&self, value: &ManifestValue) -> Result<ManifestValue, ManifestRuntimeError> {
        match self {
            Self::Plaintext(backend) => backend.pick(value),
            Self::Process(backend) => backend.pick(value),
        }
    }
}

struct CastTaskSource<'a> {
    config: &'a Config,
}

impl CastTaskSource<'_> {
    fn call_target(
        &self,
        target: &str,
        signature: &str,
        arguments: &[String],
    ) -> Result<String, DaemonError> {
        let mut args = vec!["call".to_owned(), target.to_owned(), signature.to_owned()];
        args.extend_from_slice(arguments);
        args.extend([
            "--rpc-url".to_owned(),
            self.config.rpc_url.clone(),
            "--no-proxy".to_owned(),
        ]);
        run_cast(&args).map_err(|_| DaemonError::ChainUnavailable)
    }

    fn call(&self, signature: &str, arguments: &[String]) -> Result<String, DaemonError> {
        self.call_target(&self.config.control, signature, arguments)
    }

    fn send_target(
        &self,
        target: &str,
        signature: &str,
        arguments: &[String],
    ) -> Result<(), DaemonError> {
        let mut args = vec!["send".to_owned(), target.to_owned(), signature.to_owned()];
        args.extend_from_slice(arguments);
        args.extend([
            "--private-key".to_owned(),
            self.config.node_tx_key.clone(),
            "--rpc-url".to_owned(),
            self.config.rpc_url.clone(),
            "--no-proxy".to_owned(),
            "--quiet".to_owned(),
        ]);
        run_cast(&args)
            .map(|_| ())
            .map_err(|_| DaemonError::ChainUnavailable)
    }

    fn send(&self, signature: &str, arguments: &[String]) -> Result<(), DaemonError> {
        self.send_target(&self.config.control, signature, arguments)
    }

    fn u64_call(&self, signature: &str, arguments: &[String]) -> Result<u64, DaemonError> {
        self.call(signature, arguments)?
            .parse()
            .map_err(|_| DaemonError::InvalidChainResponse)
    }

    fn status(&self, execution: &str) -> Result<ChainExecutionStatus, DaemonError> {
        let value = self.u64_call("executionStatus(bytes32)(uint8)", &[execution.to_owned()])?;
        ChainExecutionStatus::from_u8(
            u8::try_from(value).map_err(|_| DaemonError::InvalidChainResponse)?,
        )
    }

    fn advance_to_running(
        &self,
        execution: &str,
        status: ChainExecutionStatus,
    ) -> Result<ChainExecutionStatus, DaemonError> {
        match status {
            ChainExecutionStatus::Requested => {
                self.send(
                    "assignCommittee(bytes32,bytes32)",
                    &[
                        execution.to_owned(),
                        self.config.active_committee_id.clone(),
                    ],
                )?;
                self.send("markRunning(bytes32)", &[execution.to_owned()])?;
                Ok(ChainExecutionStatus::Running)
            }
            ChainExecutionStatus::CommitteeAssigned => {
                self.send("markRunning(bytes32)", &[execution.to_owned()])?;
                Ok(ChainExecutionStatus::Running)
            }
            _ => Ok(status),
        }
    }

    fn storage_root(&self) -> Result<String, DaemonError> {
        let encoded = run_cast(&[
            "abi-encode".to_owned(),
            "f(address[])".to_owned(),
            format!("[{}]", self.config.node_address),
        ])
        .map_err(|_| DaemonError::InvalidChainResponse)?;
        Ok(hex(&keccak(&decode_hex_bytes(&encoded)?)))
    }

    fn user_key_id(&self, owner: &str) -> Result<String, DaemonError> {
        let owner = decode_hex::<20>(owner)?;
        Ok(hex(&keccak_parts(&[b"PPSC_DEV_FHE_KEY_V1", &owner])))
    }

    fn ensure_user_key(&self, owner: &str) -> Result<String, DaemonError> {
        let key_id = self.user_key_id(owner)?;
        let status = self.u64_call("dataStatus(bytes32)(uint8)", std::slice::from_ref(&key_id))?;
        if status == 0 {
            let storage_root = self.storage_root()?;
            let public_key_root = hex(&keccak_parts(&[
                b"PPSC_DEV_PUBLIC_KEY_SET_V1",
                self.config.node_address.as_bytes(),
            ]));
            self.send(
                "registerDataFor(address,bytes32,bytes32,bytes32,bytes32,bytes32,uint8,uint16,uint64,uint64)",
                &[
                    owner.to_owned(),
                    key_id.clone(),
                    key_id.clone(),
                    public_key_root,
                    storage_root,
                    zero_bytes32().to_owned(),
                    "0".to_owned(),
                    "1".to_owned(),
                    "1".to_owned(),
                    "1".to_owned(),
                ],
            )?;
            self.send(
                "registerDataLocations(bytes32,address[])",
                &[key_id.clone(), format!("[{}]", self.config.node_address)],
            )?;
        }
        Ok(key_id)
    }

    fn submit_pending_upload(&self, store: &PostgresManifestState) -> Result<bool, DaemonError> {
        let Some(record) = store.next_pending_upload(&self.config.control)? else {
            return Ok(false);
        };
        let data_id = hex(&record.data_id);
        let owner = hex(&record.owner);
        let submit = (|| {
            let status =
                self.u64_call("dataStatus(bytes32)(uint8)", std::slice::from_ref(&data_id))?;
            if status == 0 {
                let (representation, threshold, fhe_key_id) = match record.value.value_type() {
                    ValueType::FheUint => (1_u8, 0_u16, self.ensure_user_key(&owner)?),
                    ValueType::Sint => (0_u8, 1_u16, zero_bytes32().to_owned()),
                    _ => return Err(DaemonError::InvalidOutput),
                };
                let public_key_root = hex(&keccak_parts(&[
                    b"PPSC_MANIFEST_UPLOAD_PK_V1",
                    self.config.node_address.as_bytes(),
                ]));
                let storage_root = self.storage_root()?;
                self.send(
                    "registerDataFor(address,bytes32,bytes32,bytes32,bytes32,bytes32,uint8,uint16,uint64,uint64)",
                    &[
                        owner.clone(),
                        data_id.clone(),
                        hex(&record.commitment),
                        public_key_root,
                        storage_root,
                        fhe_key_id,
                        representation.to_string(),
                        threshold.to_string(),
                        "1".to_owned(),
                        "1".to_owned(),
                    ],
                )?;
            }

            // Registration and location publication are separate transactions.
            // Check the latter independently so a crash between them is recoverable.
            let locations = self.call(
                "dataStorageNodes(bytes32)(address[])",
                std::slice::from_ref(&data_id),
            )?;
            if locations.trim() == "[]" {
                self.send(
                    "registerDataLocations(bytes32,address[])",
                    &[data_id.clone(), format!("[{}]", self.config.node_address)],
                )?;
            }
            Ok(())
        })();

        match submit {
            Ok(()) => {
                store.mark_upload_registered(&self.config.control, record.data_id)?;
                println!("uploaded input registered on chain: dataId={data_id} owner={owner}");
                Ok(true)
            }
            Err(error) => {
                let _ = store.mark_upload_failed(
                    &self.config.control,
                    record.data_id,
                    "chain registration failed",
                );
                Err(error)
            }
        }
    }

    fn submit_pending(&self, store: &PostgresManifestState) -> Result<bool, DaemonError> {
        let Some(record) = store.next_pending_outbox()? else {
            return Ok(false);
        };
        let decoded = decode_outbox_payload(&record.result_payload)?;
        if decoded.execution_id != record.execution_id {
            return Err(DaemonError::InvalidOutput);
        }
        let execution = hex(&record.execution_id);
        let requester = self.call(
            "executionRequester(bytes32)(address)",
            std::slice::from_ref(&execution),
        )?;
        let mut representation = 0_u8;
        let mut fhe_key_id = zero_bytes32().to_owned();
        if !decoded.state_writes.is_empty() {
            if decoded
                .state_writes
                .iter()
                .all(|write| write.value.value_type() == ValueType::FheUint)
            {
                representation = 1;
                fhe_key_id = self.ensure_user_key(&requester)?;
            } else if !decoded
                .state_writes
                .iter()
                .all(|write| write.value.value_type() == ValueType::Sint)
            {
                return Err(DaemonError::InvalidOutput);
            }
        }
        let output_id = hex(&keccak_parts(&[
            b"PPSC_MANIFEST_OUTPUT_V1",
            &record.execution_id,
            &record.result_payload,
        ]));
        let output_commitment = hex(&keccak(&record.result_payload));
        let public_key_root = hex(&keccak_parts(&[
            b"PPSC_MANIFEST_OUTPUT_PK_V1",
            self.config.node_address.as_bytes(),
        ]));
        let storage_root = self.storage_root()?;
        let old_root = self.call(
            "executionOldStateRoot(bytes32)(bytes32)",
            std::slice::from_ref(&execution),
        )?;
        let old_root_bytes = decode_hex::<32>(&old_root)?;
        let commitment_bytes = decode_hex::<32>(&output_commitment)?;
        let new_state_root = hex(&derive_state_root(
            old_root_bytes,
            commitment_bytes,
            !decoded.state_writes.is_empty(),
        ));
        let transcript_root = hex(&keccak_parts(&[
            b"PPSC_MANIFEST_TRANSCRIPT_V1",
            &record.execution_id,
            &record.result_payload,
        ]));
        let threshold = if representation == 0 { 1 } else { 0 };
        let result = format!(
            "({output_id},{output_commitment},{public_key_root},{storage_root},{fhe_key_id},{representation},{threshold},1,{new_state_root},{transcript_root})"
        );
        if self.status(&execution)? != ChainExecutionStatus::Completed {
            let digest = self.call(
                "resultDigest(bytes32,(bytes32,bytes32,bytes32,bytes32,bytes32,uint8,uint16,uint64,bytes32,bytes32))(bytes32)",
                &[execution.clone(), result.clone()],
            )?;
            let signature = sign(&digest, &self.config.node_tx_key)?;
            self.send(
                "submitResult(bytes32,(bytes32,bytes32,bytes32,bytes32,bytes32,uint8,uint16,uint64,bytes32,bytes32),bytes[])",
                &[execution.clone(), result, format!("[{signature}]")],
            )?;
        }

        let mut existing_updates = Vec::new();
        for (slot, write) in decoded.state_writes.iter().enumerate() {
            let variable = self.variable_address(&write.state, &write.key)?;
            let exists = self.call(
                "stateVariableExists(bytes32)(bool)",
                std::slice::from_ref(&variable),
            )? == "true";
            if exists {
                existing_updates.push((variable, slot));
            } else {
                self.send(
                    "declareStateVariable(bytes32,bytes32,bytes32,uint32)",
                    &[
                        hex(&self.config.contract_id),
                        variable,
                        output_id.clone(),
                        slot.to_string(),
                    ],
                )?;
            }
        }
        if !existing_updates.is_empty()
            && self.call(
                "executionVariableUpdatesFinalized(bytes32)(bool)",
                std::slice::from_ref(&execution),
            )? != "true"
        {
            let updates = format!(
                "[{}]",
                existing_updates
                    .iter()
                    .map(|(variable, slot)| format!("({variable},{slot})"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let digest = self.call(
                "variableUpdateDigest(bytes32,(bytes32,uint32)[])(bytes32)",
                &[execution.clone(), updates.clone()],
            )?;
            let signature = sign(&digest, &self.config.node_tx_key)?;
            self.send(
                "updateStateVariablesAfterResult(bytes32,(bytes32,uint32)[],bytes[])",
                &[execution.clone(), updates, format!("[{signature}]")],
            )?;
        }
        if let Some(opening) = decoded.opening {
            self.send_target(
                &self.config.gateway,
                "fulfillOpening(bytes32,bytes)",
                &[execution.clone(), hex(opening.backend_bytes())],
            )?;
        }
        store.mark_outbox_submitted(
            &self.config.control,
            &self.config.node_id,
            record.execution_id,
        )?;
        println!("result finalized on chain: execution={execution} output={output_id}");
        Ok(true)
    }

    fn variable_address(&self, state: &str, key: &[u8]) -> Result<String, DaemonError> {
        match key.len() {
            0 => self.call_target(
                &self.config.gateway,
                &format!("{state}Variable()(bytes32)"),
                &[],
            ),
            20 => self.call_target(
                &self.config.gateway,
                &format!("{state}Variable(address)(bytes32)"),
                &[hex(key)],
            ),
            _ => Err(DaemonError::InvalidOutput),
        }
    }
}

impl ManifestTaskSource for CastTaskSource<'_> {
    fn execution_count(&self) -> Result<u64, DaemonError> {
        self.u64_call("executionCount()(uint256)", &[])
    }

    fn task_at(&self, index: u64) -> Result<ManifestChainTask, DaemonError> {
        let execution_hex = self.call("executionIdAt(uint256)(bytes32)", &[index.to_string()])?;
        let arguments = vec![execution_hex.clone()];
        let contract_id =
            decode_hex(&self.call("executionContract(bytes32)(bytes32)", &arguments)?)?;
        let selector = decode_hex(&self.call("executionSelector(bytes32)(bytes4)", &arguments)?)?;
        let requester =
            decode_hex(&self.call("executionRequester(bytes32)(address)", &arguments)?)?;
        let public_inputs =
            decode_hex_bytes(&self.call("executionPublicInputs(bytes32)(bytes)", &arguments)?)?;
        let status = self.advance_to_running(&execution_hex, self.status(&execution_hex)?)?;
        let input_count = self.u64_call("executionInputCount(bytes32)(uint256)", &arguments)?;
        let mut private_input_ids = Vec::with_capacity(
            usize::try_from(input_count).map_err(|_| DaemonError::InvalidChainResponse)?,
        );
        for input_index in 0..input_count {
            private_input_ids.push(decode_hex(&self.call(
                "executionInputAt(bytes32,uint256)(bytes32)",
                &[execution_hex.clone(), input_index.to_string()],
            )?)?);
        }
        let committee = self.call("executionCommittee(bytes32)(bytes32)", &arguments)?;
        let node_is_current_member = committee != zero_bytes32()
            && self.call(
                "isCommitteeMember(bytes32,address)(bool)",
                &[committee, self.config.node_address.clone()],
            )? == "true";
        Ok(ManifestChainTask {
            execution_id: decode_hex(&execution_hex)?,
            contract_id,
            selector,
            requester,
            private_input_ids,
            public_inputs,
            status,
            node_is_current_member,
        })
    }
}

fn required(name: &str) -> Result<String, Box<dyn Error>> {
    env::var(name).map_err(|_| format!("missing environment variable {name}").into())
}

fn run_cast(args: &[String]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("cast").args(args).output()?;
    if !output.status.success() {
        return Err(format!("cast failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn sign(digest: &str, private_key: &str) -> Result<String, DaemonError> {
    run_cast(&[
        "wallet".to_owned(),
        "sign".to_owned(),
        "--no-hash".to_owned(),
        "--private-key".to_owned(),
        private_key.to_owned(),
        digest.to_owned(),
    ])
    .map_err(|_| DaemonError::ChainUnavailable)
}

fn verify_user_signature(owner: &str, digest: &str, signature: &str) -> bool {
    run_cast(&[
        "wallet".to_owned(),
        "verify".to_owned(),
        "--no-hash".to_owned(),
        "--address".to_owned(),
        owner.to_owned(),
        digest.to_owned(),
        signature.to_owned(),
    ])
    .is_ok()
}

fn handle_upload_connection(
    mut stream: TcpStream,
    store: &PostgresManifestState,
    control_text: &str,
    control: [u8; 20],
    contract_id: [u8; 32],
) {
    let result = (|| -> Result<UploadResponse, String> {
        let request = read_http_upload(&mut stream)?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "system clock error".to_owned())?
            .as_secs();
        if request.deadline < now {
            return Err("upload authorization expired".to_owned());
        }
        let validated = request
            .validate_payload(control, contract_id)
            .map_err(|_| "invalid upload".to_owned())?;
        let digest = request
            .signing_digest(control, contract_id)
            .map_err(|_| "invalid upload".to_owned())?;
        if !verify_user_signature(&request.owner, &upload_hex(&digest), &request.signature) {
            return Err("invalid owner signature".to_owned());
        }
        store
            .enqueue_upload(
                control_text,
                validated.owner,
                request.nonce,
                request.deadline,
                validated.data_id,
                validated.commitment,
                &validated.value,
            )
            .map_err(|_| "upload nonce or data id already used".to_owned())?;
        Ok(UploadResponse {
            data_id: upload_hex(&validated.data_id),
            status: "accepted".to_owned(),
        })
    })();
    match result {
        Ok(response) => {
            let _ = write_http_json(&mut stream, "202 Accepted", &response);
        }
        Err(message) => {
            let _ = write_http_json(&mut stream, "400 Bad Request", &json!({ "error": message }));
        }
    }
}

fn start_upload_listener(config: &Config) -> Result<(), Box<dyn Error>> {
    let Some(listen) = &config.upload_listen else {
        return Ok(());
    };
    let listener = TcpListener::bind(listen)?;
    let store = PostgresManifestState::connect(&config.database_url, config.contract_id)?;
    let control_text = config.control.clone();
    let control = decode_fixed::<20>(&config.control)?;
    let contract_id = config.contract_id;
    let listen = listen.clone();
    thread::spawn(move || {
        println!("authenticated input upload listening: http://{listen}/v1/inputs");
        for connection in listener.incoming() {
            match connection {
                Ok(stream) => {
                    handle_upload_connection(stream, &store, &control_text, control, contract_id)
                }
                Err(error) => eprintln!("upload listener failed to accept connection: {error}"),
            }
        }
    });
    Ok(())
}

fn zero_bytes32() -> &'static str {
    "0x0000000000000000000000000000000000000000000000000000000000000000"
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::from("0x");
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    let mut digest = [0_u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    hasher.finalize(&mut digest);
    digest
}

fn keccak_parts(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Keccak::v256();
    for part in parts {
        hasher.update(part);
    }
    let mut digest = [0_u8; 32];
    hasher.finalize(&mut digest);
    digest
}

fn derive_state_root(
    old_state_root: [u8; 32],
    output_commitment: [u8; 32],
    has_state_writes: bool,
) -> [u8; 32] {
    if !has_state_writes {
        // A read-only opening must not advance contract state. This also
        // allows independent queries created from one root to finish in any order.
        return old_state_root;
    }
    keccak_parts(&[
        b"PPSC_MANIFEST_STATE_V1",
        &old_state_root,
        &output_commitment,
    ])
}

fn verify_manifest(
    chain: &CastTaskSource<'_>,
    manifest: &ContractManifest,
) -> Result<(), Box<dyn Error>> {
    let canonical = serde_json::to_string_pretty(manifest)?;
    let local = hex(&keccak(canonical.as_bytes()));
    let on_chain = chain.call(
        "contractManifestHash(bytes32)(bytes32)",
        &[hex(&chain.config.contract_id)],
    )?;
    if local != on_chain.to_ascii_lowercase() {
        return Err(format!("manifest hash mismatch: local={local} chain={on_chain}").into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;
    let manifest = manifest_from_json(&fs::read_to_string(&config.manifest_path)?)?;
    let store = PostgresManifestState::connect(&config.database_url, config.contract_id)?;
    let chain = CastTaskSource { config: &config };
    verify_manifest(&chain, &manifest)?;

    let args: Vec<String> = env::args().collect();
    start_upload_listener(&config)?;

    let backend = DaemonCryptoBackend::load(&config)?;
    let daemon = ManifestCommitteeDaemon::new(
        &chain,
        &store,
        &backend,
        &manifest,
        config.contract_id,
        &config.control,
        &config.node_id,
    );
    let once = args.get(1).map(String::as_str) == Some("once");
    println!(
        "manifest committee daemon started: node={} contract={} backend={} poll={}s",
        config.node_id,
        hex(&config.contract_id),
        config.crypto_backend,
        config.poll_interval.as_secs()
    );
    loop {
        if let DaemonCryptoBackend::Process(process) = &backend {
            process.ensure_running()?;
        }
        match daemon.poll_once() {
            Ok(PollOutcome::Idle) => {}
            Ok(PollOutcome::Waiting) => println!("waiting for committee assignment or handoff"),
            Ok(PollOutcome::Skipped {
                index,
                execution_id,
            }) => {
                println!(
                    "task skipped: index={index} execution={}",
                    hex(&execution_id)
                );
            }
            Ok(PollOutcome::Computed {
                index,
                execution_id,
            }) => println!(
                "task computed and queued: index={index} execution={}",
                hex(&execution_id)
            ),
            Err(error) => eprintln!("poll failed; cursor retained for retry: {error}"),
        }
        if let Err(error) = chain.submit_pending(&store) {
            eprintln!("outbox submission failed; retained for retry: {error}");
        }
        if let Err(error) = chain.submit_pending_upload(&store) {
            eprintln!("input registration failed; retained for retry: {error}");
        }
        if once {
            break;
        }
        thread::sleep(config.poll_interval);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::derive_state_root;

    #[test]
    fn read_only_opening_keeps_state_root() {
        let old = [7_u8; 32];
        assert_eq!(derive_state_root(old, [9_u8; 32], false), old);
        assert_ne!(derive_state_root(old, [9_u8; 32], true), old);
    }
}
