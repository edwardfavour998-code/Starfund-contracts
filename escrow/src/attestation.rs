//! Private attestation storage and validation helpers.
//!
//! This module centralizes the storage layout and bounds/validation logic for
//! attestation append, lookup, revoke and unrevoke operations. Public wrappers
//! remain in `lib.rs` and delegate here so that storage representations and the
//! public ABI are preserved.

use soroban_sdk::{Address, Env, Vec};

use crate::DataKey;

/// Shared bounds check used by both single and batch attestation operations.
///
/// Ensures `index` is a valid position within the append log of length `len`.
/// Panics with a stable message when the index is out of range so that single
/// and batch operations behave identically.
pub(crate) fn require_attestation_index_in_range(index: u32, len: u32) {
    if index >= len {
        panic!("attestation index out of range");
    }
}

/// Returns the current attestation append log.
pub(crate) fn get_append_log(env: &Env) -> Vec<Address> {
    env.storage()
        .persistent()
        .get(&DataKey::AttestationAppendLog)
        .unwrap_or_else(|| Vec::new(env))
}

/// Persists the attestation append log.
fn set_append_log(env: &Env, log: &Vec<Address>) {
    env.storage()
        .persistent()
        .set(&DataKey::AttestationAppendLog, log);
}

/// Appends a single attestation entry to the log and returns its index.
pub(crate) fn append(env: &Env, attestor: &Address) -> u32 {
    let mut log = get_append_log(env);
    let index = log.len();
    log.push_back(attestor.clone());
    set_append_log(env, &log);
    index
}

/// Appends a batch of attestation entries to the log and returns the index of
/// the first appended entry.
pub(crate) fn append_batch(env: &Env, attestors: &Vec<Address>) -> u32 {
    let mut log = get_append_log(env);
    let start = log.len();
    for attestor in attestors.iter() {
        log.push_back(attestor);
    }
    set_append_log(env, &log);
    start
}

/// Looks up the attestor stored at `index`, validating the index first.
pub(crate) fn lookup(env: &Env, index: u32) -> Address {
    let log = get_append_log(env);
    require_attestation_index_in_range(index, log.len());
    log.get(index).unwrap()
}

/// Returns whether the attestation at `index` is currently revoked.
pub(crate) fn is_revoked(env: &Env, index: u32) -> bool {
    env.storage()
        .persistent()
        .get(&DataKey::AttestationRevoked(index))
        .unwrap_or(false)
}

/// Revokes a single attestation entry, validating the index first.
pub(crate) fn revoke(env: &Env, index: u32) {
    let log = get_append_log(env);
    require_attestation_index_in_range(index, log.len());
    env.storage()
        .persistent()
        .set(&DataKey::AttestationRevoked(index), &true);
}

/// Revokes a batch of attestation entries, validating each index first.
pub(crate) fn revoke_batch(env: &Env, indices: &Vec<u32>) {
    let log = get_append_log(env);
    let len = log.len();
    for index in indices.iter() {
        require_attestation_index_in_range(index, len);
    }
    for index in indices.iter() {
        env.storage()
            .persistent()
            .set(&DataKey::AttestationRevoked(index), &true);
    }
}

/// Unrevokes a single attestation entry, validating the index first.
pub(crate) fn unrevoke(env: &Env, index: u32) {
    let log = get_append_log(env);
    require_attestation_index_in_range(index, log.len());
    env.storage()
        .persistent()
        .set(&DataKey::AttestationRevoked(index), &false);
}

/// Unrevokes a batch of attestation entries, validating each index first.
pub(crate) fn unrevoke_batch(env: &Env, indices: &Vec<u32>) {
    let log = get_append_log(env);
    let len = log.len();
    for index in indices.iter() {
        require_attestation_index_in_range(index, len);
    }
    for index in indices.iter() {
        env.storage()
            .persistent()
            .set(&DataKey::AttestationRevoked(index), &false);
    }
}