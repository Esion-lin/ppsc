//! P1: multi-committee seed-generation benchmark for the real PRSS `Π_ResharePair`.
//!
//! Measures, for honest-majority committees of increasing size, the number of holder sets,
//! the number of PRSS seeds (`|H1| × |H2| = C(n,t)^2`), the per-party seed storage, the
//! wall-clock generation time, and the total seed bytes. This validates the `O(C(n,t)^2)`
//! combinatorial blow-up the advisor asked to check (vs. the dealer shortcut `reshare_pair_dealer`).

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

fn main() {
    // Honest-majority thresholds t = floor((n-1)/2). Start at the advisor's 5-6 member
    // committees, then grow to show the blow-up.
    let configs: [(usize, usize); 8] = [
        (5, 2),
        (6, 2),
        (7, 3),
        (8, 3),
        (9, 4),
        (10, 4),
        (12, 5),
        (14, 6),
    ];

    println!("n,t,holder_sets,seed_count,seed_per_party,time_ms,total_bytes");
    for (n, t) in configs {
        let src = Committee::<Fr>::new(t, n).expect("src");
        let dst = Committee::<Fr>::new(t, n).expect("dst");
        let mut rng = StdRng::seed_from_u64(0);

        let holders = combination(n as u64, t as u64);
        let seed_count = holders * holders;
        let seed_per_party = combination((n - 1) as u64, t as u64);

        let start = Instant::now();
        let _mask = reshare_pair(&src, t, &dst, t, b"bench", &mut rng).expect("reshare");
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        let total_bytes = seed_count * 32;

        println!("{n},{t},{holders},{seed_count},{seed_per_party},{elapsed:.2},{total_bytes}");
    }
}
