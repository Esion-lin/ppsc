//! Extended seed-generation experiments for the real PRSS `Π_ResharePair`.
//!
//! Three experiments cover the advisor's checklist (member count, threshold, holder/seed counts,
//! wall-clock, per-party storage, communication, blow-up):
//!   1. fixed small threshold t=2, handoff `Π^{t,t}_ResharePair` (polynomial growth, no blow-up)
//!   2. honest-majority t=floor((n-1)/2), handoff `Π^{t,t}_ResharePair` (O(C(n,t)^2) blow-up)
//!   3. honest-majority, multiplication `Π^{2t,t}_ResharePair` (degree-2t source side)

use ark_bn254::Fr;
use ppsc_crypto::mpc::{reshare_pair, Committee};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::time::Instant;

fn combination(n: u64, k: u64) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut result = 1_u64;
    for i in 0..k {
        result = result * (n - i) / (i + 1);
    }
    result
}

fn run(experiment: &str, n: usize, t: usize, src_degree: usize, dst_degree: usize) {
    let src = Committee::<Fr>::new(t, n).expect("src");
    let dst = Committee::<Fr>::new(t, n).expect("dst");
    let mut rng = StdRng::seed_from_u64(0);

    // holder_sets(n, degree) = {A : |A| = n - degree}, so |H| = C(n, n - degree).
    let hs_src = combination(n as u64, (n - src_degree) as u64);
    let hs_dst = combination(n as u64, (n - dst_degree) as u64);
    let seed_count = hs_src * hs_dst;
    // Per-party seed storage: a party belongs to C(n-1, n-degree-1) = C(n-1, degree) holder sets.
    let seed_per_party_src = combination((n - 1) as u64, src_degree as u64);
    let seed_per_party_dst = combination((n - 1) as u64, dst_degree as u64);

    let start = Instant::now();
    let _mask =
        reshare_pair(&src, src_degree, &dst, dst_degree, b"bench", &mut rng).expect("reshare");
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    // Communication/storage: each of the |H1| x |H2| seeds is 32 bytes (PRSS seed).
    let total_bytes = seed_count * 32;

    println!(
        "{experiment},{n},{t},{src_degree},{dst_degree},{hs_src},{hs_dst},{seed_count},{seed_per_party_src},{seed_per_party_dst},{elapsed:.2},{total_bytes}"
    );
}

fn main() {
    println!("experiment,n,t,src_degree,dst_degree,holder_sets_src,holder_sets_dst,seed_count,seed_per_party_src,seed_per_party_dst,time_ms,total_bytes");

    // 1. fixed t=2, handoff (t,t)
    for n in [5_usize, 8, 16, 32, 64] {
        run("fixed_t2_handoff", n, 2, 2, 2);
    }

    // 2. honest-majority t=floor((n-1)/2), handoff (t,t)
    for n in [5_usize, 7, 9, 11, 13] {
        let t = (n - 1) / 2;
        run("honest_majority_handoff", n, t, t, t);
    }

    // 3. honest-majority, multiplication (2t, t)
    for n in [5_usize, 7, 9, 11, 13] {
        let t = (n - 1) / 2;
        run("honest_majority_mult", n, t, 2 * t, t);
    }
}
