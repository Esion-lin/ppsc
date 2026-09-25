//! 阶段 B：多轮 Π + Handoff 数值闭环。
//!
//! 展示导师主例子：
//!   ([x]^t, [y]^t) --Π--> [z]^{2t} --handoff + degree reduction--> [z]^t
//!
//! 执行边界（导师 06:33-10:33 的结论）：
//!   - 多轮协议 Π 在 **committee ℓ 内部**完成（本地乘 → degree-2t，不 handoff）；
//!   - 只在 **epoch 边界** 一次性 handoff，用 **degree-2t correlated mask** 交给 committee ℓ+1，
//!     同时 **degree-reduce 回 degree-t**；
//!   - 这里只验证**数值正确性**（重构 = x·y），不含恶意 verification（阶段 C）。

use ark_bn254::Fr;
use ppsc_crypto::mpc::{
    apply_difference, reconstruct_from_parts, reshare_pair_dealer, Committee, ShamirShare,
};
use rand::rngs::StdRng;
use rand::SeedableRng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut rng = StdRng::seed_from_u64(0xF0F0);
    let t = 2;
    // committee ℓ 与 ℓ+1：诚实多数，n=7 > 2t=4。
    let comm_old = Committee::<Fr>::new(t, 7)?;
    let comm_new = Committee::<Fr>::new(t, 7)?;

    // committee ℓ 持有 [x]^t, [y]^t
    let x = Fr::from(6_u64);
    let y = Fr::from(7_u64);
    let x_shares = comm_old.split(x, &mut rng)?;
    let y_shares = comm_old.split(y, &mut rng)?;

    println!("== committee ℓ: in-committee Π (multiplication), no handoff ==");

    // 阶段 1：committee ℓ 内完成 Π —— 各 party 本地乘得到 degree-2t 的 [z]。
    // 注意：不在这里做 degree reduction、不 handoff；多轮协议（如 S2B 的 33 次打开）
    // 也同理只在委员会内部交互。
    let z_2t: Vec<ShamirShare<Fr>> = x_shares
        .iter()
        .zip(y_shares.iter())
        .map(|(xi, yi)| ShamirShare::from_point_value(xi.point(), xi.value() * yi.value()))
        .collect();
    println!(
        "  [z]^{{2t}} = [x]·[y] computed locally (degree {}, in-committee)",
        2 * t
    );

    println!("== epoch boundary: handoff + degree reduction to committee ℓ+1 ==");

    // 阶段 2：epoch 边界用 degree-2t correlated mask 交给新 committee，并恢复 degree-t。
    let mask = reshare_pair_dealer(&comm_old, 2 * t, &comm_new, t, &mut rng)?;
    let masked: Vec<ShamirShare<Fr>> = z_2t
        .iter()
        .zip(mask.source.iter())
        .map(|(zi, ri)| ShamirShare::from_point_value(zi.point(), zi.value() - ri.value()))
        .collect();
    let points: Vec<Fr> = masked.iter().map(|s| s.point()).collect();
    let values: Vec<Fr> = masked.iter().map(|s| s.value()).collect();
    let delta = reconstruct_from_parts(&points, &values, 2 * t + 1)?;
    let z_new = apply_difference(delta, &mask.destination);
    println!(
        "  dealer opened masked diff δ = z - r (degree {}), broadcast to ℓ+1",
        2 * t
    );

    // 阶段 3：新 committee 重构 [z]^t。
    println!("== committee ℓ+1: reconstructs [z]^t ==");
    let result = comm_new.reconstruct(&z_new)?;
    assert_eq!(result, x * y);
    println!("  x·y = {x}·{y} = {result}");
    println!("OK: in-committee multi-round computation + one boundary handoff preserves x·y");
    Ok(())
}
