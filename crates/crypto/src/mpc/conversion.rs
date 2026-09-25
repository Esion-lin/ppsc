//! FHE↔SS representation conversion (paper §3.1 Case 2) — **algebraic reference implementation**.
//!
//! A simplified LWE ciphertext `⟦x⟧=(a,b)` with `b = a·s + x·Δ + e` (`s` is the threshold-FHE
//! secret key, `Δ` the public scale, `e` the noise). H2S uses a correlated mask
//! `([r·Δ]^{src}, [r]^{dst})` to turn the ciphertext into a degree-`t` sharing of `dst`, and the
//! dealer applies `⌊·/Δ⌋` scale+round decoding on the reconstructed value; S2H uses
//! `([r]^{src}, ⟦r⟧)` to turn a sharing into a public ciphertext. This module is only an
//! algebraic skeleton (no concrete FHE library) for matching the paper's formulas and verifying
//! the MPC-side algebra.
//!
//! **Production implementations (real OpenFHE ciphertexts) live in the `ppsc-fhe` crate:**
//! - CKKS single-key H2S/S2H: `ppsc_fhe::hybrid::{fhe_to_sharing, sharing_to_fhe}`
//! - BFV/BGV plaintext-space H2S/S2H: `ppsc_fhe::bfv_shamir::{bfv_share_plaintext, bfv_shares_to_ciphertext}`

use super::auth::AuthenticatedSharing;
use super::handoff::masked_difference_shares;
use super::prss::reshare_pair_small;
use super::shamir::{reconstruct, Committee, ShamirShare};
use super::MpcError;
use ark_ff::{BigInteger, PrimeField};
use rand::Rng;

/// A simplified LWE ciphertext (public, copyable): `b = a·s + x·Δ + e`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FheCiphertext<F: PrimeField> {
    pub a: F,
    pub b: F,
}

/// H2S correlated mask: `([r·Δ]^{src}, [r]^{dst})`.
pub struct H2sMask<F: PrimeField> {
    pub source_scaled: Vec<ShamirShare<F>>,
    pub destination: Vec<ShamirShare<F>>,
}

/// S2H correlated mask: `([r]^{src}, ⟦r⟧)`.
pub struct S2hMask<F: PrimeField> {
    pub source: Vec<ShamirShare<F>>,
    pub ciphertext: FheCiphertext<F>,
}

fn small<F: PrimeField>(n: u64) -> F {
    F::from_le_bytes_mod_order(&n.to_le_bytes())
}

/// Encrypt with the full secret key `s` (for trusted/offline preprocessing; online protocols only touch ciphertexts).
pub fn encrypt<F: PrimeField>(
    plaintext: F,
    key: F,
    delta: u64,
    noise: F,
    rng: &mut impl Rng,
) -> FheCiphertext<F> {
    let a = F::rand(rng);
    FheCiphertext {
        a,
        b: a * key + plaintext * small::<F>(delta) + noise,
    }
}

/// Decrypt with the full secret key `s` (test/illustration only; production uses threshold decryption).
pub fn decrypt<F: PrimeField>(ct: &FheCiphertext<F>, key: F, delta: u64) -> F {
    (ct.b - ct.a * key) / small::<F>(delta)
}

/// Generate an H2S mask: PRSS distributively generates a correlated pair
/// `([r]^{src}, [r]^{dst})` within the plaintext range, and the source locally multiplies by `Δ`
/// to get `[r·Δ]`. `r` is the "plaintext mask" (paper §3.1); each component `ζ_{i,j}` is a small
/// integer in `[0, bound)`, so `r = Σ ζ` stays in the plaintext range to keep `(x-r)·Δ` from
/// wrapping, while no single party knows `r` (paper §3.2 PRSS).
pub fn h2s_mask<F: PrimeField>(
    src: &Committee<F>,
    dst: &Committee<F>,
    delta: u64,
    bound: u64,
    nonce: &[u8],
    rng: &mut impl Rng,
) -> Result<H2sMask<F>, MpcError> {
    let pair = reshare_pair_small(src, src.threshold, dst, dst.threshold, nonce, bound, rng)?;
    let scale = small::<F>(delta);
    Ok(H2sMask {
        source_scaled: pair
            .source
            .iter()
            .map(|s| ShamirShare::new(s.point(), scale * s.value()))
            .collect(),
        destination: pair.destination,
    })
}

/// Generate an S2H mask: a small random `r` in the plaintext range, shared as `[r]^{src}` and encrypted as `⟦r⟧`.
pub fn s2h_mask<F: PrimeField>(
    src: &Committee<F>,
    key: F,
    delta: u64,
    rng: &mut impl Rng,
) -> Result<S2hMask<F>, MpcError> {
    let r = small::<F>(rng.next_u64() & 0xFFFF_FFFF);
    Ok(S2hMask {
        source: src.split(r, rng)?,
        ciphertext: encrypt(r, key, delta, F::zero(), rng),
    })
}

/// `Π_h2s`: public ciphertext `⟦x⟧` + threshold key shares `[s]` → degree-`t` sharing `[x]` of `dst`.
///
/// The dealer reconstructs the masked decryption intermediate `c = (x-r)·Δ + e`, then decodes via `⌊c/Δ⌋` (noise removal).
pub fn h2s<F: PrimeField>(
    ct: &FheCiphertext<F>,
    key_shares: &[ShamirShare<F>],
    mask: &H2sMask<F>,
    src: &Committee<F>,
    dst: &Committee<F>,
    delta: u64,
) -> Result<Vec<ShamirShare<F>>, MpcError> {
    if key_shares.len() != mask.source_scaled.len() || key_shares.len() != src.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    if mask.destination.len() != dst.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    let c_shares: Vec<ShamirShare<F>> = key_shares
        .iter()
        .zip(mask.source_scaled.iter())
        .map(|(s, r)| {
            if s.point() != r.point() {
                return Err(MpcError::InconsistentShares);
            }
            Ok(ShamirShare::new(
                s.point(),
                ct.b - ct.a * s.value() - r.value(),
            ))
        })
        .collect::<Result<_, MpcError>>()?;
    let c = reconstruct(&c_shares, src.threshold + 1)?;
    let delta_prime = decode_mask(c, delta)?;
    Ok(mask
        .destination
        .iter()
        .map(|r| ShamirShare::new(r.point(), r.value() + delta_prime))
        .collect())
}

/// `Π_s2h`: degree-`t` sharing `[x]` → public ciphertext `⟦x⟧`.
pub fn s2h<F: PrimeField>(
    x_shares: &[ShamirShare<F>],
    mask: &S2hMask<F>,
    src: &Committee<F>,
    delta: u64,
) -> Result<FheCiphertext<F>, MpcError> {
    if x_shares.len() != mask.source.len() || x_shares.len() != src.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    let diff_shares = masked_difference_shares(x_shares, &mask.source)?;
    let diff = reconstruct(&diff_shares, src.threshold + 1)?;
    Ok(FheCiphertext {
        a: mask.ciphertext.a,
        b: mask.ciphertext.b + diff * small::<F>(delta),
    })
}

/// `Π_auth-h2s`: public ciphertext `⟦x⟧` → authenticated sharing `⟨x⟩ = ([x],[γ_x])` of `dst`.
///
/// The value path reuses the H2S reconstruction/decoding; the MAC path adds
/// `[γ_x] = [γ_r] + δ̂·[α]`, where `[γ_r]` is the MAC tag of the same mask `r` (offline:
/// `[γ_r]=[αr]`) and `δ̂` is the opened masked difference.
#[allow(clippy::too_many_arguments)]
pub fn authenticated_h2s<F: PrimeField>(
    ct: &FheCiphertext<F>,
    key_shares: &[ShamirShare<F>],
    mask: &H2sMask<F>,
    mac_mask: &[ShamirShare<F>],
    alpha: &[ShamirShare<F>],
    src: &Committee<F>,
    dst: &Committee<F>,
    delta: u64,
) -> Result<AuthenticatedSharing<F>, MpcError> {
    if key_shares.len() != mask.source_scaled.len() || key_shares.len() != src.size() {
        return Err(MpcError::ShareCountMismatch);
    }
    if mask.destination.len() != dst.size()
        || mac_mask.len() != dst.size()
        || alpha.len() != dst.size()
    {
        return Err(MpcError::ShareCountMismatch);
    }
    let c_shares: Vec<ShamirShare<F>> = key_shares
        .iter()
        .zip(mask.source_scaled.iter())
        .map(|(s, r)| {
            if s.point() != r.point() {
                return Err(MpcError::InconsistentShares);
            }
            Ok(ShamirShare::new(
                s.point(),
                ct.b - ct.a * s.value() - r.value(),
            ))
        })
        .collect::<Result<_, MpcError>>()?;
    let c = reconstruct(&c_shares, src.threshold + 1)?;
    let delta_prime = decode_mask(c, delta)?;

    let value: Vec<ShamirShare<F>> = mask
        .destination
        .iter()
        .map(|r| ShamirShare::new(r.point(), r.value() + delta_prime))
        .collect();
    let mac: Vec<ShamirShare<F>> = mac_mask
        .iter()
        .zip(alpha.iter())
        .map(|(g, a)| ShamirShare::new(g.point(), g.value() + delta_prime * a.value()))
        .collect();
    Ok(AuthenticatedSharing::new(value, mac))
}

/// `Π_auth-s2h`: authenticated sharing `⟨x⟩` → public ciphertext `⟦x⟧`, with a MAC check.
///
/// Opens `δ = x - r` and `v = γ_δ - δ·α`; proceeds only if `v = 0` (the MAC relation holds),
/// then outputs `⟦x⟧ = (a, b + Δ·Encode(δ))`.
pub fn authenticated_s2h<F: PrimeField>(
    x: &AuthenticatedSharing<F>,
    mask: &S2hMask<F>,
    mac_mask: &[ShamirShare<F>],
    alpha: &[ShamirShare<F>],
    src: &Committee<F>,
    delta: u64,
) -> Result<FheCiphertext<F>, MpcError> {
    if x.value.len() != mask.source.len()
        || x.value.len() != src.size()
        || x.mac.len() != mac_mask.len()
        || mac_mask.len() != alpha.len()
    {
        return Err(MpcError::ShareCountMismatch);
    }
    let diff = masked_difference_shares(&x.value, &mask.source)?;
    let gamma_diff = masked_difference_shares(&x.mac, mac_mask)?;
    let delta_bar = reconstruct(&diff, src.threshold + 1)?;

    let v_shares: Vec<ShamirShare<F>> = gamma_diff
        .iter()
        .zip(alpha.iter())
        .map(|(g, a)| ShamirShare::new(g.point(), g.value() - delta_bar * a.value()))
        .collect();
    let v = reconstruct(&v_shares, src.threshold + 1)?;
    if v != F::zero() {
        return Err(MpcError::MacCheckFailed);
    }

    Ok(FheCiphertext {
        a: mask.ciphertext.a,
        b: mask.ciphertext.b + delta_bar * small::<F>(delta),
    })
}

/// scale+round decode of the masked decryption intermediate `c = (x-r)·Δ + e` via `⌊·/Δ⌋`.
///
/// First centered-lift `c` to `(-Q/2, Q/2)`, then floor-divide by `Δ`. Assumes `c` lies in the
/// centered range (`|e| < Δ/2` and no wrap); full wrap handling is threshold-FHE-specific.
fn decode_mask<F: PrimeField>(masked: F, delta: u64) -> Result<F, MpcError> {
    let centered = field_to_centered_i128(masked)?;
    let scaled = floor_div(centered, delta as i128);
    Ok(i128_to_field::<F>(scaled))
}

fn field_to_centered_i128<F: PrimeField>(x: F) -> Result<i128, MpcError> {
    let big = x.into_bigint();
    if big > F::MODULUS_MINUS_ONE_DIV_TWO {
        let mut mag = F::MODULUS;
        let _borrow = mag.sub_with_borrow(&big);
        Ok(-bigint_to_i128::<F>(mag)?)
    } else {
        bigint_to_i128::<F>(big)
    }
}

fn bigint_to_i128<F: PrimeField>(big: F::BigInt) -> Result<i128, MpcError> {
    let bytes = big.to_bytes_le();
    if bytes.len() > 16 && bytes[16..].iter().any(|&b| b != 0) {
        return Err(MpcError::RangeCheckFailed);
    }
    let mut buf = [0_u8; 16];
    let n = bytes.len().min(16);
    buf[..n].copy_from_slice(&bytes[..n]);
    Ok(i128::from_le_bytes(buf))
}

fn i128_to_field<F: PrimeField>(v: i128) -> F {
    if v >= 0 {
        F::from_le_bytes_mod_order(&v.to_le_bytes())
    } else {
        let mag = v.unsigned_abs();
        -F::from_le_bytes_mod_order(&mag.to_le_bytes())
    }
}

fn floor_div(a: i128, b: i128) -> i128 {
    let q = a / b;
    let r = a % b;
    if r != 0 && (a < 0) != (b < 0) {
        q - 1
    } else {
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_bn254::Fr;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng() -> StdRng {
        StdRng::seed_from_u64(0x0A0B_0C0D)
    }

    const DELTA: u64 = 1_000_003;

    #[test]
    fn h2s_decodes_noisy_ciphertext() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let dst = Committee::<Fr>::new(2, 5).expect("dst");
        let key = Fr::from(29_u64);
        let x = Fr::from(1234_u64);
        let noise = Fr::from(123_u64);
        let ct = encrypt(x, key, DELTA, noise, &mut rng());
        let key_shares = src.split(key, &mut rng()).expect("split key");
        let mask = h2s_mask(&src, &dst, DELTA, 1 << 32, b"h2s", &mut rng()).expect("mask");

        let shares = h2s(&ct, &key_shares, &mask, &src, &dst, DELTA).expect("h2s");
        assert_eq!(dst.reconstruct(&shares).expect("reconstruct"), x);
    }

    #[test]
    fn s2h_converts_sharing_to_ciphertext() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let key = Fr::from(31_u64);
        let x = Fr::from(4321_u64);
        let x_shares = src.split(x, &mut rng()).expect("split x");
        let mask = s2h_mask(&src, key, DELTA, &mut rng()).expect("mask");

        let ct = s2h(&x_shares, &mask, &src, DELTA).expect("s2h");
        assert_eq!(decrypt(&ct, key, DELTA), x);
    }

    #[test]
    fn s2h_then_h2s_round_trips() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let dst = Committee::<Fr>::new(2, 5).expect("dst");
        let key = Fr::from(37_u64);
        let x = Fr::from(999_u64);
        let x_shares = src.split(x, &mut rng()).expect("split x");
        let s2h_mask = s2h_mask(&src, key, DELTA, &mut rng()).expect("s2h mask");
        let ct = s2h(&x_shares, &s2h_mask, &src, DELTA).expect("s2h");

        let key_shares = src.split(key, &mut rng()).expect("split key");
        let h2s_mask = h2s_mask(&src, &dst, DELTA, 1 << 32, b"h2s", &mut rng()).expect("h2s mask");
        let back = h2s(&ct, &key_shares, &h2s_mask, &src, &dst, DELTA).expect("h2s");
        assert_eq!(dst.reconstruct(&back).expect("reconstruct"), x);
    }

    #[test]
    fn authenticated_h2s_produces_value_and_mac() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let dst = Committee::<Fr>::new(2, 5).expect("dst");
        let key = Fr::from(29_u64);
        let alpha = Fr::from(7_u64);
        let x = Fr::from(1234_u64);
        let noise = Fr::from(123_u64);
        let ct = encrypt(x, key, DELTA, noise, &mut rng());
        let key_shares = src.split(key, &mut rng()).expect("split key");
        let mask = h2s_mask(&src, &dst, DELTA, 1 << 32, b"ah2s", &mut rng()).expect("mask");
        // Offline: [γ_r] = [α·r] for the same r held in mask.destination.
        let r = dst.reconstruct(&mask.destination).expect("mask r");
        let mac_mask = dst.split(r * alpha, &mut rng()).expect("mac mask");
        let alpha_shares = dst.split(alpha, &mut rng()).expect("alpha");

        let out = authenticated_h2s(
            &ct,
            &key_shares,
            &mask,
            &mac_mask,
            &alpha_shares,
            &src,
            &dst,
            DELTA,
        )
        .expect("auth h2s");
        assert_eq!(dst.reconstruct(&out.value).expect("v"), x);
        assert_eq!(dst.reconstruct(&out.mac).expect("m"), x * alpha);
    }

    #[test]
    fn authenticated_s2h_checks_mac_and_emits_ciphertext() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let key = Fr::from(31_u64);
        let alpha = Fr::from(11_u64);
        let x = Fr::from(4321_u64);
        let auth_x = AuthenticatedSharing::new(
            src.split(x, &mut rng()).expect("x value"),
            src.split(x * alpha, &mut rng()).expect("x mac"),
        );
        let alpha_shares = src.split(alpha, &mut rng()).expect("alpha");
        let mask = s2h_mask(&src, key, DELTA, &mut rng()).expect("mask");
        let r = src.reconstruct(&mask.source).expect("mask r");
        let mac_mask = src.split(r * alpha, &mut rng()).expect("mac mask");

        let ct = authenticated_s2h(&auth_x, &mask, &mac_mask, &alpha_shares, &src, DELTA)
            .expect("auth s2h");
        assert_eq!(decrypt(&ct, key, DELTA), x);
    }

    #[test]
    fn authenticated_s2h_rejects_tampered_mac() {
        let src = Committee::<Fr>::new(2, 5).expect("src");
        let key = Fr::from(31_u64);
        let alpha = Fr::from(11_u64);
        let x = Fr::from(4321_u64);
        let auth_x = AuthenticatedSharing::new(
            src.split(x, &mut rng()).expect("x value"),
            src.split(x * alpha + Fr::from(1_u64), &mut rng())
                .expect("bad mac"),
        );
        let alpha_shares = src.split(alpha, &mut rng()).expect("alpha");
        let mask = s2h_mask(&src, key, DELTA, &mut rng()).expect("mask");
        let r = src.reconstruct(&mask.source).expect("mask r");
        let mac_mask = src.split(r * alpha, &mut rng()).expect("mac mask");

        let result = authenticated_s2h(&auth_x, &mask, &mac_mask, &alpha_shares, &src, DELTA);
        assert!(matches!(result, Err(MpcError::MacCheckFailed)));
    }
}
