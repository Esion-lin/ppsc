//! 阶段 B（续）：多轮协议 S2B 在 committee 内完成 + 边界 handoff。
//!
//! `Π_S2B` 是一个多轮协议（33 次「打开」，dealer 来回参与），全部在 committee ℓ 内部完成、
//! 不每轮 handoff；输出是 x 的 33 个布尔分享（degree-t）。epoch 边界一次性 handoff 到
//! committee ℓ+1，重构出原值 x。只验证数值正确，不含恶意 verification（阶段 C）。

use ark_bn254::Fr;
use ppsc_crypto::mpc::{
    basic_handoff, generate_bit_extract_pair_bounded, reshare_pair_dealer, s2b_bounded, Committee,
    ShamirShare, F2,
};
use rand::rngs::StdRng;
use rand::SeedableRng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::seed_from_u64(0x5E2B);
    let t = 2;
    let bit_len = 32;

    let comm_f_old = Committee::<Fr>::new(t, 5)?;
    let comm_b_old = Committee::<F2>::new(t, 5)?;
    let comm_b_new = Committee::<F2>::new(t, 5)?;

    let x = Fr::from(42_u64);
    let x_shares = comm_f_old.split(x, &mut rng)?;

    println!("== committee ℓ: in-committee multi-round Π_S2B ({bit_len} opens), no handoff ==");
    let pair = generate_bit_extract_pair_bounded(&comm_f_old, &comm_b_old, bit_len, &mut rng)?;
    let bits = s2b_bounded(
        &x_shares,
        &pair,
        bit_len,
        &comm_f_old,
        &comm_b_old,
        &mut rng,
    )?;
    println!("  [x] decomposed into {bit_len} boolean shares (degree-t, in-committee)");

    println!("== epoch boundary: handoff {bit_len} boolean shares to committee ℓ+1 ==");
    let handed: Vec<Vec<ShamirShare<F2>>> = bits
        .iter()
        .map(|bit| {
            let mask = reshare_pair_dealer(&comm_b_old, t, &comm_b_new, t, &mut rng)?;
            basic_handoff(bit, &mask, &comm_b_old, &comm_b_new)
        })
        .collect::<Result<Vec<_>, _>>()?;
    println!("  handed {bit_len} boolean shares to ℓ+1 (degree-t)");

    println!("== committee ℓ+1: reconstructs bits of x ==");
    let mut recovered = Fr::from(0_u64);
    let mut two_pow = Fr::from(1_u64);
    for bit in &handed {
        if comm_b_new.reconstruct(bit)? == F2(1) {
            recovered += two_pow;
        }
        two_pow += two_pow;
    }
    assert_eq!(recovered, x);
    println!("  recovered x = {recovered} (from {bit_len} boolean shares)");
    println!("OK: multi-round S2B in-committee + boundary handoff preserves x");
    Ok(())
}
