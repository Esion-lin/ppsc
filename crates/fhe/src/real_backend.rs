//! Real `ConfidentialBackend` implementation: BFV (Multiparty) ciphertexts + `Fr` Shamir
//! sharings + MPC comparison. This is the production-style backend that replaces
//! `PlaintextBackend`, used by the real-password demo of deposit / transfer / query.
//!
//! It still runs all parties in one process (3 Multiparty keys, Shamir t=2/n=5); a real
//! deployment must split the roles onto nodes and coordinate them via the protocol engine.

use ark_bn254::Fr;
use ark_ff::{BigInteger, PrimeField};
use ppsc_compiler::ValueType;
use ppsc_core::Commitment;
use ppsc_crypto::mpc::{
    basic_handoff, conditional_transfer_strictly_greater, generate_bit_extract_pair,
    greater_than_or_equal, reshare_pair_dealer, Committee, ShamirShare, F2,
};
use ppsc_runtime::manifest::{ManifestCryptoBackend, ManifestRuntimeError, ManifestValue};
use ppsc_runtime::{
    AssetId, BackendError, CommitmentBackend, FheAmount, FheBackend, HybridConversionBackend,
    MpcAmount, MpcBackend, PlaintextBackend, PrivateAccountId,
};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::sync::Mutex;

use crate::bfv_shamir::bfv_share_plaintext;
use crate::{Ciphertext, CkksContext, KeyPair};

const BATCH: usize = 8;
const THRESHOLD: usize = 2;
const PARTIES: usize = 5;

pub struct FheMpcBackend {
    ctx: CkksContext,
    party_keys: Vec<KeyPair>,
    committee_f: Committee<Fr>,
    committee_b: Committee<F2>,
    rng: Mutex<StdRng>,
}

fn field_to_int(f: Fr) -> i64 {
    f.into_bigint().as_ref()[0] as i64
}

fn serialize_shares(shares: &[ShamirShare<Fr>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + shares.len() * 64);
    out.extend_from_slice(&(shares.len() as u64).to_le_bytes());
    for s in shares {
        out.extend_from_slice(&s.point().into_bigint().to_bytes_le());
        out.extend_from_slice(&s.value().into_bigint().to_bytes_le());
    }
    out
}

fn deserialize_shares(bytes: &[u8]) -> Option<Vec<ShamirShare<Fr>>> {
    if bytes.len() < 8 {
        return None;
    }
    let n = u64::from_le_bytes(bytes[..8].try_into().ok()?) as usize;
    if bytes.len() != 8 + n * 64 {
        return None;
    }
    let mut shares = Vec::with_capacity(n);
    for i in 0..n {
        let off = 8 + i * 64;
        let point = Fr::from_le_bytes_mod_order(&bytes[off..off + 32]);
        let value = Fr::from_le_bytes_mod_order(&bytes[off + 32..off + 64]);
        shares.push(ShamirShare::from_point_value(point, value));
    }
    Some(shares)
}

fn serialize_bool_shares(shares: &[ShamirShare<F2>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + shares.len() * 2);
    out.extend_from_slice(&(shares.len() as u64).to_le_bytes());
    for share in shares {
        out.push(share.point().0);
        out.push(share.value().0);
    }
    out
}

fn deserialize_bool_shares(bytes: &[u8]) -> Option<Vec<ShamirShare<F2>>> {
    if bytes.len() < 8 {
        return None;
    }
    let count = u64::from_le_bytes(bytes[..8].try_into().ok()?) as usize;
    if bytes.len() != 8 + count * 2 {
        return None;
    }
    Some(
        (0..count)
            .map(|index| {
                let offset = 8 + index * 2;
                ShamirShare::from_point_value(F2(bytes[offset]), F2(bytes[offset + 1]))
            })
            .collect(),
    )
}

fn commit_digest(domain: &[u8], parts: &[&[u8]]) -> Commitment {
    let mut output = [0_u8; 32];
    for lane in 0..4_u64 {
        let mut state = 0xcbf2_9ce4_8422_2325_u64 ^ lane;
        for byte in domain
            .iter()
            .copied()
            .chain(parts.iter().flat_map(|part| part.iter().copied()))
        {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
        output[(lane as usize) * 8..(lane as usize + 1) * 8].copy_from_slice(&state.to_be_bytes());
    }
    Commitment::from_bytes(output)
}

impl FheMpcBackend {
    pub fn new() -> Self {
        let ctx = CkksContext::bfv_new(2, BATCH as u32).expect("bfv context");
        let mut party_keys = vec![ctx.multiparty_keygen_first().expect("kp1")];
        for _ in 1..3 {
            let kp = ctx
                .multiparty_keygen_next_kp(party_keys.last().expect("last kp"))
                .expect("next kp");
            party_keys.push(kp);
        }
        let committee_f = Committee::<Fr>::new(THRESHOLD, PARTIES).expect("committee f");
        let committee_b = Committee::<F2>::new(THRESHOLD, PARTIES).expect("committee b");
        let rng = Mutex::new(StdRng::from_entropy());
        Self {
            ctx,
            party_keys,
            committee_f,
            committee_b,
            rng,
        }
    }

    fn agg_key(&self) -> &KeyPair {
        self.party_keys.last().expect("aggregate key")
    }

    /// Export only the aggregate public key and public crypto context.
    pub fn public_key(&self) -> Result<Vec<u8>, BackendError> {
        self.agg_key().serialize_pubkey().ok_or(BackendError::InvalidEncoding)
    }

    fn encrypt_single(&self, value: i64) -> Result<FheAmount, BackendError> {
        let mut data = vec![0_i64; BATCH];
        data[0] = value;
        let ct = self
            .ctx
            .bfv_encrypt_int(self.agg_key(), &data)
            .ok_or(BackendError::InvalidCiphertext)?;
        let bytes = ct.serialize().ok_or(BackendError::InvalidCiphertext)?;
        Ok(FheAmount::from_bytes(bytes))
    }

    fn decrypt_single(&self, fhe: &FheAmount) -> Result<u128, BackendError> {
        let ct = Ciphertext::deserialize(&self.ctx, fhe.as_bytes())
            .ok_or(BackendError::InvalidCiphertext)?;
        let dec = self
            .ctx
            .bfv_multiparty_decrypt(&ct, &self.party_keys, BATCH)
            .ok_or(BackendError::InvalidCiphertext)?;
        Ok(dec.first().copied().unwrap_or(0) as u128)
    }

    /// Hand the live `MpcAmount` (a Shamir sharing over the current committee) to
    /// `new_committee` via `Π_Handoff`. This is the runtime's committee-rotation entry point:
    /// at an epoch boundary it pulls a cross-committee mask from offline preprocessing and
    /// transfers the live SS state without reconstructing the secret.
    pub fn handoff_share(
        &self,
        mpc: &MpcAmount,
        new_committee: &Committee<Fr>,
    ) -> Result<MpcAmount, BackendError> {
        let shares =
            deserialize_shares(mpc.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let mut rng = self.rng.lock().expect("rng");
        let mask = reshare_pair_dealer(
            &self.committee_f,
            self.committee_f.threshold,
            new_committee,
            new_committee.threshold,
            &mut *rng,
        )
        .map_err(|_| BackendError::InvalidEncoding)?;
        let new_shares = basic_handoff(&shares, &mask, &self.committee_f, new_committee)
            .map_err(|_| BackendError::InvalidEncoding)?;
        Ok(MpcAmount::from_backend_bytes(serialize_shares(&new_shares)))
    }

    /// Reconstruct a live `MpcAmount` under `committee` (authorized opening; demo only).
    pub fn reconstruct_mpc(
        &self,
        mpc: &MpcAmount,
        committee: &Committee<Fr>,
    ) -> Result<u128, BackendError> {
        let shares =
            deserialize_shares(mpc.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let value = committee
            .reconstruct(&shares)
            .map_err(|_| BackendError::InvalidEncoding)?;
        Ok(field_to_int(value) as u128)
    }

    fn eval_fhe_binary(
        &self,
        left: &FheAmount,
        right: &FheAmount,
        subtract: bool,
    ) -> Result<FheAmount, BackendError> {
        let left = Ciphertext::deserialize(&self.ctx, left.as_bytes())
            .ok_or(BackendError::InvalidCiphertext)?;
        let right = Ciphertext::deserialize(&self.ctx, right.as_bytes())
            .ok_or(BackendError::InvalidCiphertext)?;
        let output = if subtract {
            self.ctx.eval_sub(&left, &right)
        } else {
            self.ctx.eval_add(&left, &right)
        }
        .ok_or(BackendError::InvalidCiphertext)?;
        Ok(FheAmount::from_bytes(
            output.serialize().ok_or(BackendError::InvalidCiphertext)?,
        ))
    }

    fn secret_ge(&self, left: &MpcAmount, right: &MpcAmount) -> Result<Vec<u8>, BackendError> {
        let left = deserialize_shares(left.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let right =
            deserialize_shares(right.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let mut rng = self.rng.lock().expect("rng");
        let pair = generate_bit_extract_pair(&self.committee_f, &self.committee_b, &mut *rng)
            .map_err(|_| BackendError::InvalidEncoding)?;
        let shares = greater_than_or_equal(
            &left,
            &right,
            &pair,
            &self.committee_f,
            &self.committee_b,
            &mut *rng,
        )
        .map_err(|_| BackendError::InvalidEncoding)?;
        Ok(serialize_bool_shares(&shares))
    }

    fn open_secret_bool(&self, bytes: &[u8]) -> Result<bool, BackendError> {
        let shares = deserialize_bool_shares(bytes).ok_or(BackendError::InvalidEncoding)?;
        let bit = self
            .committee_b
            .reconstruct(&shares)
            .map_err(|_| BackendError::InvalidEncoding)?;
        Ok(bit == F2(1))
    }
}

impl Default for FheMpcBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MpcBackend for FheMpcBackend {
    fn share_amount(&self, amount: u128) -> Result<MpcAmount, BackendError> {
        let mut rng = self.rng.lock().expect("rng");
        let shares = self
            .committee_f
            .split(Fr::from(amount as u64), &mut *rng)
            .map_err(|_| BackendError::InvalidEncoding)?;
        Ok(MpcAmount::from_backend_bytes(serialize_shares(&shares)))
    }

    fn checked_add(&self, left: &MpcAmount, right: &MpcAmount) -> Result<MpcAmount, BackendError> {
        let l = deserialize_shares(left.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let r = deserialize_shares(right.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let sum: Vec<ShamirShare<Fr>> = l
            .iter()
            .zip(r.iter())
            .map(|(a, b)| ShamirShare::from_point_value(a.point(), a.value() + b.value()))
            .collect();
        Ok(MpcAmount::from_backend_bytes(serialize_shares(&sum)))
    }

    fn checked_sub(&self, left: &MpcAmount, right: &MpcAmount) -> Result<MpcAmount, BackendError> {
        let l = deserialize_shares(left.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let r = deserialize_shares(right.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let diff: Vec<ShamirShare<Fr>> = l
            .iter()
            .zip(r.iter())
            .map(|(a, b)| ShamirShare::from_point_value(a.point(), a.value() - b.value()))
            .collect();
        Ok(MpcAmount::from_backend_bytes(serialize_shares(&diff)))
    }

    fn greater_than_or_equal(
        &self,
        left: &MpcAmount,
        right: &MpcAmount,
    ) -> Result<bool, BackendError> {
        let l = deserialize_shares(left.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let r = deserialize_shares(right.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let mut rng = self.rng.lock().expect("rng");
        let pair = generate_bit_extract_pair(&self.committee_f, &self.committee_b, &mut *rng)
            .map_err(|_| BackendError::InvalidEncoding)?;
        let geq = greater_than_or_equal(
            &l,
            &r,
            &pair,
            &self.committee_f,
            &self.committee_b,
            &mut *rng,
        )
        .map_err(|_| BackendError::InvalidEncoding)?;
        let bit = self
            .committee_b
            .reconstruct(&geq)
            .map_err(|_| BackendError::InvalidEncoding)?;
        Ok(bit == F2(1))
    }

    fn conditional_transfer_strictly_greater(
        &self,
        sender: &MpcAmount,
        receiver: &MpcAmount,
        minimum: &MpcAmount,
        amount: &MpcAmount,
    ) -> Result<(MpcAmount, MpcAmount), BackendError> {
        let s = deserialize_shares(sender.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let r =
            deserialize_shares(receiver.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let m = deserialize_shares(minimum.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let a = deserialize_shares(amount.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let mut rng = self.rng.lock().expect("rng");
        let pair = generate_bit_extract_pair(&self.committee_f, &self.committee_b, &mut *rng)
            .map_err(|_| BackendError::InvalidEncoding)?;
        let (s_new, r_new) = conditional_transfer_strictly_greater(
            &s,
            &r,
            &m,
            &a,
            &pair,
            &self.committee_f,
            &self.committee_b,
            &mut *rng,
        )
        .map_err(|_| BackendError::InvalidEncoding)?;
        Ok((
            MpcAmount::from_backend_bytes(serialize_shares(&s_new)),
            MpcAmount::from_backend_bytes(serialize_shares(&r_new)),
        ))
    }
}

impl FheBackend for FheMpcBackend {
    fn encrypt_amount(
        &self,
        amount: u128,
        _account: PrivateAccountId,
        _asset: AssetId,
    ) -> Result<FheAmount, BackendError> {
        if amount > crate::BFV_MAX_AMOUNT {
            return Err(BackendError::Overflow);
        }
        self.encrypt_single(amount as i64)
    }

    fn decrypt_for_owner(
        &self,
        ciphertext: &FheAmount,
        _account: PrivateAccountId,
        authorization: &[u8],
    ) -> Result<u128, BackendError> {
        if authorization != PlaintextBackend::authorization_for_tests() {
            return Err(BackendError::Unauthorized);
        }
        self.decrypt_single(ciphertext)
    }
}

impl HybridConversionBackend for FheMpcBackend {
    fn fhe_to_mpc(&self, ciphertext: &FheAmount) -> Result<MpcAmount, BackendError> {
        let ct = Ciphertext::deserialize(&self.ctx, ciphertext.as_bytes())
            .ok_or(BackendError::InvalidCiphertext)?;
        let mut rng = self.rng.lock().expect("rng");
        let shares = bfv_share_plaintext(
            &self.ctx,
            &ct,
            &self.party_keys,
            &self.committee_f,
            1,
            &mut *rng,
        )
        .ok_or(BackendError::InvalidCiphertext)?;
        let single = shares
            .into_iter()
            .next()
            .ok_or(BackendError::InvalidEncoding)?;
        Ok(MpcAmount::from_backend_bytes(serialize_shares(&single)))
    }

    fn mpc_to_fhe(
        &self,
        value: &MpcAmount,
        _account: PrivateAccountId,
        _asset: AssetId,
    ) -> Result<FheAmount, BackendError> {
        let shares =
            deserialize_shares(value.backend_bytes()).ok_or(BackendError::InvalidEncoding)?;
        let plaintext = self
            .committee_f
            .reconstruct(&shares)
            .map_err(|_| BackendError::InvalidEncoding)?;
        self.encrypt_single(field_to_int(plaintext))
    }
}

impl CommitmentBackend for FheMpcBackend {
    fn commit(&self, domain: &[u8], parts: &[&[u8]]) -> Commitment {
        commit_digest(domain, parts)
    }
}

impl ManifestCryptoBackend for FheMpcBackend {
    fn evaluate(
        &self,
        opcode: &str,
        arguments: &[ManifestValue],
    ) -> Result<ManifestValue, ManifestRuntimeError> {
        match opcode {
            "fhe.encrypt" => {
                let value = public_u128(one(arguments)?)?;
                let encrypted = self
                    .encrypt_amount(value, manifest_account(), manifest_asset())
                    .map_err(manifest_backend_error)?;
                ManifestValue::protected(ValueType::FheUint, encrypted.as_bytes().to_vec())
            }
            "fhe.add" | "fhe.sub" => {
                let (left, right) = two(arguments)?;
                require_type(left, ValueType::FheUint)?;
                require_type(right, ValueType::FheUint)?;
                let output = self
                    .eval_fhe_binary(
                        &FheAmount::from_bytes(left.backend_bytes().to_vec()),
                        &FheAmount::from_bytes(right.backend_bytes().to_vec()),
                        opcode == "fhe.sub",
                    )
                    .map_err(manifest_backend_error)?;
                ManifestValue::protected(ValueType::FheUint, output.as_bytes().to_vec())
            }
            "fhe.ge" => {
                let (left, right) = two(arguments)?;
                require_type(left, ValueType::FheUint)?;
                require_type(right, ValueType::FheUint)?;
                let left = self
                    .fhe_to_mpc(&FheAmount::from_bytes(left.backend_bytes().to_vec()))
                    .map_err(manifest_backend_error)?;
                let right = self
                    .fhe_to_mpc(&FheAmount::from_bytes(right.backend_bytes().to_vec()))
                    .map_err(manifest_backend_error)?;
                ManifestValue::protected(
                    ValueType::FheBool,
                    self.secret_ge(&left, &right)
                        .map_err(manifest_backend_error)?,
                )
            }
            "mpc.add" | "mpc.sub" => {
                let (left, right) = two(arguments)?;
                require_type(left, ValueType::Sint)?;
                require_type(right, ValueType::Sint)?;
                let left = MpcAmount::from_backend_bytes(left.backend_bytes().to_vec());
                let right = MpcAmount::from_backend_bytes(right.backend_bytes().to_vec());
                let output = if opcode == "mpc.add" {
                    self.checked_add(&left, &right)
                } else {
                    self.checked_sub(&left, &right)
                }
                .map_err(manifest_backend_error)?;
                ManifestValue::protected(ValueType::Sint, output.backend_bytes().to_vec())
            }
            "mpc.ge" => {
                let (left, right) = two(arguments)?;
                require_type(left, ValueType::Sint)?;
                require_type(right, ValueType::Sint)?;
                ManifestValue::protected(
                    ValueType::SecretBool,
                    self.secret_ge(
                        &MpcAmount::from_backend_bytes(left.backend_bytes().to_vec()),
                        &MpcAmount::from_backend_bytes(right.backend_bytes().to_vec()),
                    )
                    .map_err(manifest_backend_error)?,
                )
            }
            "convert.h2s" => {
                let value = one(arguments)?;
                match value.value_type() {
                    ValueType::FheUint => {
                        let output = self
                            .fhe_to_mpc(&FheAmount::from_bytes(value.backend_bytes().to_vec()))
                            .map_err(manifest_backend_error)?;
                        ManifestValue::protected(ValueType::Sint, output.backend_bytes().to_vec())
                    }
                    ValueType::FheBool => ManifestValue::protected(
                        ValueType::SecretBool,
                        value.backend_bytes().to_vec(),
                    ),
                    _ => Err(ManifestRuntimeError::TypeMismatch),
                }
            }
            "convert.s2h" => {
                let value = one(arguments)?;
                require_type(value, ValueType::Sint)?;
                let output = self
                    .mpc_to_fhe(
                        &MpcAmount::from_backend_bytes(value.backend_bytes().to_vec()),
                        manifest_account(),
                        manifest_asset(),
                    )
                    .map_err(manifest_backend_error)?;
                ManifestValue::protected(ValueType::FheUint, output.as_bytes().to_vec())
            }
            _ => Err(ManifestRuntimeError::UnsupportedOperator),
        }
    }

    fn secret_bool(&self, value: &ManifestValue) -> Result<bool, ManifestRuntimeError> {
        require_type(value, ValueType::SecretBool)?;
        self.open_secret_bool(value.backend_bytes())
            .map_err(manifest_backend_error)
    }

    fn pick(&self, value: &ManifestValue) -> Result<ManifestValue, ManifestRuntimeError> {
        require_type(value, ValueType::Sint)?;
        let amount = self
            .reconstruct_mpc(
                &MpcAmount::from_backend_bytes(value.backend_bytes().to_vec()),
                &self.committee_f,
            )
            .map_err(manifest_backend_error)?;
        let mut opened = b"PPSC_MANIFEST_OPENED_U128_V1".to_vec();
        opened.extend_from_slice(&amount.to_be_bytes());
        ManifestValue::protected(ValueType::Opened, opened)
    }
}

fn manifest_account() -> PrivateAccountId {
    PrivateAccountId::from_bytes([0_u8; 32])
}

fn manifest_asset() -> AssetId {
    AssetId::from_bytes([0_u8; 20])
}

fn manifest_backend_error(error: BackendError) -> ManifestRuntimeError {
    match error {
        BackendError::Overflow => ManifestRuntimeError::Overflow,
        BackendError::InsufficientBalance => ManifestRuntimeError::Underflow,
        _ => ManifestRuntimeError::InvalidValue,
    }
}

fn one(arguments: &[ManifestValue]) -> Result<&ManifestValue, ManifestRuntimeError> {
    if arguments.len() != 1 {
        return Err(ManifestRuntimeError::InvalidManifest);
    }
    Ok(&arguments[0])
}

fn two(
    arguments: &[ManifestValue],
) -> Result<(&ManifestValue, &ManifestValue), ManifestRuntimeError> {
    if arguments.len() != 2 {
        return Err(ManifestRuntimeError::InvalidManifest);
    }
    Ok((&arguments[0], &arguments[1]))
}

fn require_type(value: &ManifestValue, expected: ValueType) -> Result<(), ManifestRuntimeError> {
    if value.value_type() != expected {
        return Err(ManifestRuntimeError::TypeMismatch);
    }
    Ok(())
}

fn public_u128(value: &ManifestValue) -> Result<u128, ManifestRuntimeError> {
    require_type(value, ValueType::Uint)?;
    let bytes: [u8; 16] = value
        .backend_bytes()
        .try_into()
        .map_err(|_| ManifestRuntimeError::InvalidValue)?;
    Ok(u128::from_be_bytes(bytes))
}
