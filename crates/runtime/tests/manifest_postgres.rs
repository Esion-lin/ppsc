use ppsc_compiler::compile;
use ppsc_runtime::manifest::{
    InvocationContext, InvocationInputs, ManifestExecutor, ManifestStateStore,
    PlaintextManifestBackend, PostgresManifestState, TaskOutcome,
};
use ppsc_runtime::manifest_upload::hex;
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_contract() -> [u8; 32] {
    let mut id = [0xC7; 32];
    id[..16].copy_from_slice(
        &SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
            .to_be_bytes(),
    );
    id
}

#[test]
fn manifest_state_and_daemon_cursor_survive_restart() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration test: TEST_DATABASE_URL is not set");
        return;
    };
    let contract_id = unique_contract();
    let control = format!("control-{}", hex(&contract_id));
    let sender = [0x11; 20];
    let manifest = compile(
        r#"
        privacy contract Counter {
          private FheUint balance[address];
          function deposit(FheUint amount) public {
            balance[msg.sender] = FHE.add(balance[msg.sender], amount);
          }
        }
        "#,
    )
    .expect("compile");

    {
        let state = PostgresManifestState::connect(&database_url, contract_id).expect("connect");
        state
            .initialize("balance", &sender, &PlaintextManifestBackend::fhe_uint(10))
            .expect("initialize");
        let backend = PlaintextManifestBackend;
        let mut inputs = InvocationInputs::default();
        inputs.insert("amount", PlaintextManifestBackend::fhe_uint(5));
        let outcome = ManifestExecutor::new(&manifest, &state, &backend)
            .evaluate("deposit", &InvocationContext { sender }, &inputs)
            .expect("evaluate");
        let before_commit = state.load("balance", &sender).expect("uncommitted balance");
        assert_eq!(
            PlaintextManifestBackend::open_for_tests(&before_commit),
            Ok(10)
        );
        assert_eq!(
            state
                .next_execution_index(&control, "node-a")
                .expect("cursor"),
            0
        );
        state
            .commit_computation(
                &control,
                "node-a",
                0,
                [0xE1; 32],
                &outcome.state_writes,
                b"attested-result-payload",
            )
            .expect("atomic state, outbox and cursor commit");
        assert_eq!(
            state
                .pending_outbox_count(&control, "node-a")
                .expect("outbox count"),
            1
        );
    }

    let reopened = PostgresManifestState::connect(&database_url, contract_id).expect("reconnect");
    let balance = reopened.load("balance", &sender).expect("persistent state");
    assert_eq!(PlaintextManifestBackend::open_for_tests(&balance), Ok(15));
    assert_eq!(
        reopened
            .next_execution_index(&control, "node-a")
            .expect("persistent cursor"),
        1
    );
    assert!(reopened
        .complete_execution(&control, "node-a", 0, [0xE1; 32], TaskOutcome::Completed,)
        .is_err());
}

#[test]
fn authenticated_upload_outbox_survives_restart_and_rejects_nonce_reuse() {
    let Ok(database_url) = std::env::var("TEST_DATABASE_URL") else {
        eprintln!("skipping PostgreSQL integration test: TEST_DATABASE_URL is not set");
        return;
    };
    let contract_id = unique_contract();
    let control = format!("0xcontrol-{}", contract_id[0]);
    let owner = [0x44; 20];
    let data_id = unique_contract();
    let commitment = [0x55; 32];
    let value = PlaintextManifestBackend::fhe_uint(42);

    {
        let state = PostgresManifestState::connect(&database_url, contract_id).expect("connect");
        state
            .enqueue_upload(
                &control,
                owner,
                7,
                2_000_000_000,
                data_id,
                commitment,
                &value,
            )
            .expect("queue upload");
    }

    let state = PostgresManifestState::connect(&database_url, contract_id).expect("reconnect");
    let pending = state
        .next_pending_upload(&control)
        .expect("pending query")
        .expect("pending upload");
    assert_eq!(pending.data_id, data_id);
    assert_eq!(pending.owner, owner);
    assert_eq!(pending.commitment, commitment);
    assert_eq!(
        PlaintextManifestBackend::open_for_tests(&pending.value),
        Ok(42)
    );

    // Exact retry is idempotent, but the same owner nonce cannot authorize a
    // different ciphertext/data id.
    state
        .enqueue_upload(
            &control,
            owner,
            7,
            2_000_000_000,
            data_id,
            commitment,
            &value,
        )
        .expect("idempotent retry");
    assert!(state
        .enqueue_upload(
            &control,
            owner,
            7,
            2_000_000_000,
            [0x66; 32],
            [0x77; 32],
            &PlaintextManifestBackend::fhe_uint(43),
        )
        .is_err());
    state
        .mark_upload_registered(&control, data_id)
        .expect("mark registered");
    assert!(state
        .next_pending_upload(&control)
        .expect("pending query")
        .is_none());
}
