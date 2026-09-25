//! Cross-committee online protocols: `Π_Handoff` (basic handoff) and `Π_mult`
//! (multiplication + degree reduction + handoff).
//!
//! Both share the same dealer-mediated pattern: the source committee computes masked
//! difference shares locally, the dealer only reconstructs and broadcasts `delta`, and
//! the destination committee locally adds `delta` onto the correlated mask.

use super::field::ShareField;
use super::shamir::{reconstruct, Committee, ShamirShare};
use super::{HandoffMask, MpcError};

/// Source-side local computation of `[delta]_i = [x]_i - [mask]_i`, requiring point-wise alignment.
pub fn masked_difference_shares<F: ShareField>(
    x: &[ShamirShare<F>],
    mask: &[ShamirShare<F>],
) -> Result<Vec<ShamirShare<F>>, MpcError> {
    if x.len() != mask.len() {
        return Err(MpcError::ShareCountMismatch);
    }
    x.iter()
        .zip(mask.iter())
        .map(|(xi, ri)| {
            if xi.point() != ri.point() {
                return Err(MpcError::InconsistentShares);
            }
            Ok(ShamirShare::new(xi.point(), xi.value() - ri.value()))
        })
        .collect()
}

/// Destination-side local `[x]_i = [mask]_i + delta`.
pub fn apply_difference<F: ShareField>(delta: F, mask: &[ShamirShare<F>]) -> Vec<ShamirShare<F>> {
    mask.iter()
        .map(|r| ShamirShare::new(r.point(), r.value() + delta))
        .collect()
}

/// `Π_Handoff`: `[x]^{src} -> [x]^{dst}` — changes only the holders, not the value.
pub fn basic_handoff<F: ShareField>(
    x: &[ShamirShare<F>],
    mask: &HandoffMask<F>,
    src: &Committee<F>,
    dst: &Committee<F>,
) -> Result<Vec<ShamirShare<F>>, MpcError> {
    if x.len() != mask.source.len() || x.len() != src.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    if mask.destination.len() != dst.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    let delta_shares = masked_difference_shares(x, &mask.source)?;
    let delta = reconstruct(&delta_shares, src.threshold + 1)?;
    Ok(apply_difference(delta, &mask.destination))
}

/// `Π_mult`: `[x]^t · [y]^t` yields `[z]^t`, also performing the cross-committee handoff.
///
/// The source locally multiplies into a degree-`2t` `[delta]=xy-r`; reconstruction needs `n > 2t`.
pub fn multiply_and_handoff<F: ShareField>(
    x: &[ShamirShare<F>],
    y: &[ShamirShare<F>],
    mask: &HandoffMask<F>,
    src: &Committee<F>,
    dst: &Committee<F>,
) -> Result<Vec<ShamirShare<F>>, MpcError> {
    if x.len() != y.len() || x.len() != mask.source.len() || x.len() != src.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    if mask.destination.len() != dst.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    if src.size() <= 2 * src.threshold {
        return Err(MpcError::TooFewParties);
    }
    let delta_shares: Vec<ShamirShare<F>> = x
        .iter()
        .zip(y.iter())
        .zip(mask.source.iter())
        .map(|((xi, yi), ri)| {
            if xi.point() != yi.point() || xi.point() != ri.point() {
                return Err(MpcError::InconsistentShares);
            }
            Ok(ShamirShare::new(
                xi.point(),
                xi.value() * yi.value() - ri.value(),
            ))
        })
        .collect::<Result<_, MpcError>>()?;
    let delta = reconstruct(&delta_shares, 2 * src.threshold + 1)?;
    Ok(apply_difference(delta, &mask.destination))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mpc::{
        reconstruct_from_parts, reshare_pair, reshare_pair_dealer, HandoffMask, MpcError,
    };
    use ark_bn254::Fr;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0x1234_5678)
    }

    #[test]
    fn basic_handoff_preserves_secret_across_committees() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let dst = Committee::<Fr>::new(2, 5).expect("dst");
        let x = Fr::from(99_u64);
        let x_shares = src.split(x, &mut rng()).expect("split");
        let mask = reshare_pair(&src, 2, &dst, 2, b"ho", &mut rng()).expect("mask");

        let handed = basic_handoff(&x_shares, &mask, &src, &dst).expect("handoff");
        assert_eq!(dst.reconstruct(&handed).expect("reconstruct"), x);
    }

    #[test]
    fn multiply_and_handoff_computes_product() {
        let src = Committee::<Fr>::new(2, 7).expect("src n=7>2t=4");
        let dst = Committee::<Fr>::new(2, 7).expect("dst");
        let x = Fr::from(6_u64);
        let y = Fr::from(7_u64);
        let xs = src.split(x, &mut rng()).expect("split x");
        let ys = src.split(y, &mut rng()).expect("split y");
        let mask = reshare_pair(&src, 4, &dst, 2, b"mul", &mut rng()).expect("mask");

        let z = multiply_and_handoff(&xs, &ys, &mask, &src, &dst).expect("mult");
        assert_eq!(dst.reconstruct(&z).expect("reconstruct"), x * y);
    }

    #[test]
    fn multiply_requires_enough_parties() {
        let src = Committee::<Fr>::new(2, 4).expect("src n=4 not > 2t");
        let dst = Committee::<Fr>::new(2, 5).expect("dst");
        let xs = src.split(Fr::from(1_u64), &mut rng()).expect("split x");
        let ys = src.split(Fr::from(1_u64), &mut rng()).expect("split y");
        let mask = HandoffMask {
            source: src.split(Fr::from(0_u64), &mut rng()).expect("dummy src"),
            source_degree: 4,
            destination: dst.split(Fr::from(0_u64), &mut rng()).expect("dummy dst"),
            destination_degree: 2,
        };
        let result = multiply_and_handoff(&xs, &ys, &mask, &src, &dst);
        assert!(matches!(result, Err(MpcError::TooFewParties)));
    }

    #[test]
    fn multi_round_handoff_preserves_product_for_random_inputs() {
        use ark_ff::UniformRand;
        for (t, n) in [(1_usize, 4_usize), (2, 7), (3, 9), (5, 13)] {
            let old = Committee::<Fr>::new(t, n).expect("old");
            let new = Committee::<Fr>::new(t, n).expect("new");
            for _ in 0..20 {
                let x = UniformRand::rand(&mut rng());
                let y = UniformRand::rand(&mut rng());
                let x_shares = old.split(x, &mut rng()).expect("split x");
                let y_shares = old.split(y, &mut rng()).expect("split y");

                // 阶段 1：committee 内局部乘 → degree-2t（不 handoff）。
                let z_2t: Vec<ShamirShare<Fr>> = x_shares
                    .iter()
                    .zip(y_shares.iter())
                    .map(|(a, b)| ShamirShare::from_point_value(a.point(), a.value() * b.value()))
                    .collect();

                // 阶段 2：边界 handoff + degree reduction（degree-2t → degree-t）。
                let mask = reshare_pair_dealer(&old, 2 * t, &new, t, &mut rng()).expect("mask");
                let masked: Vec<ShamirShare<Fr>> = z_2t
                    .iter()
                    .zip(mask.source.iter())
                    .map(|(z, r)| ShamirShare::from_point_value(z.point(), z.value() - r.value()))
                    .collect();
                let pts: Vec<Fr> = masked.iter().map(|s| s.point()).collect();
                let vals: Vec<Fr> = masked.iter().map(|s| s.value()).collect();
                let delta = reconstruct_from_parts(&pts, &vals, 2 * t + 1).expect("dealer");
                let handed = apply_difference(delta, &mask.destination);

                assert_eq!(new.reconstruct(&handed).expect("reconstruct"), x * y);
            }
        }
    }
}
