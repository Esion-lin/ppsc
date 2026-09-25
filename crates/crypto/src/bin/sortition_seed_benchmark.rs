//! A: candidate-pool size does not affect seed-generation cost.
//!
//! For a growing candidate pool (100 → 100_000 nodes) we (1) sortition-select a 5-6 member
//! committee and (2) run the real PRSS `Π_ResharePair` on that committee. The point: seed
//! generation cost depends only on the committee's `n`/`t`, not on the pool size; the pool
//! only affects the (negligible) sortition sampling.

use ark_bn254::Fr;
use ppsc_crypto::mpc::{reshare_pair, Committee};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
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

const REPEAT: usize = 100;

fn main() {
    let pool_sizes: [usize; 4] = [100, 1000, 10000, 100000];
    let committee_sizes: [usize; 2] = [5, 6];
    let t = 2;

    println!("pool_size,committee_size,threshold,sortition_ms,seed_gen_ms,seed_count");
    for &pool in &pool_sizes {
        for &n in &committee_sizes {
            let mut rng = StdRng::seed_from_u64(0);
            let candidates: Vec<usize> = (0..pool).collect();

            // (1) sortition: draw `n` members from the pool, averaged over REPEAT.
            let mut sortition_total = 0.0_f64;
            for _ in 0..REPEAT {
                let start = Instant::now();
                let _selected: Vec<&usize> = candidates.choose_multiple(&mut rng, n).collect();
                sortition_total += start.elapsed().as_secs_f64() * 1000.0;
            }
            let sortition_ms = sortition_total / REPEAT as f64;

            // (2) seed generation: real PRSS `Π^{t,t}_ResharePair` on the selected committee.
            let src = Committee::<Fr>::new(t, n).expect("src");
            let dst = Committee::<Fr>::new(t, n).expect("dst");
            let mut seed_total = 0.0_f64;
            for i in 0..REPEAT {
                let nonce = i.to_le_bytes();
                let start = Instant::now();
                let _mask = reshare_pair(&src, t, &dst, t, &nonce, &mut rng).expect("reshare");
                seed_total += start.elapsed().as_secs_f64() * 1000.0;
            }
            let seed_gen_ms = seed_total / REPEAT as f64;

            let seed_count = combination(n as u64, t as u64).pow(2);

            println!("{pool},{n},{t},{sortition_ms:.6},{seed_gen_ms:.4},{seed_count}");
        }
    }
}
