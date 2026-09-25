//! Handoff 接入 runtime 的演示。
//!
//! 展示：合约执行产生 live 的 SS 状态（`MpcAmount` 分享）→ epoch 切换 → runtime 调用
//! `FheMpcBackend::handoff_share` 把状态转移到新 committee → 值不变、无单方知道明文。

use ark_bn254::Fr;
use ppsc_crypto::mpc::Committee;
use ppsc_fhe::real_backend::FheMpcBackend;
use ppsc_runtime::MpcBackend;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let backend = FheMpcBackend::new();

    // 旧 committee（epoch ℓ）上，runtime 持有一个 live 值 x=123 的分享
    let live_mpc = backend.share_amount(123)?;
    let new_committee = Committee::<Fr>::new(2, 5)?;

    println!("== epoch ℓ: active committee holds a live SS share (value hidden) ==");

    // epoch 切换：runtime 调用 Handoff，把 live 分享转移到新 committee
    let new_mpc = backend.handoff_share(&live_mpc, &new_committee)?;

    let recovered = backend.reconstruct_mpc(&new_mpc, &new_committee)?;
    println!("== epoch ℓ+1: new committee reconstructs x = {recovered} ==");
    assert_eq!(recovered, 123);
    println!("OK: value preserved across committee rotation, no single party learned x");
    Ok(())
}
