//! Callback module for the escrow contract.
//!
//! This module owns all callback-related types, storage indexing helpers,
//! internal helpers, and the public callback entrypoints. Storage keys are
//! constructed via the shared `keys` module so that on-chain storage layout
//! remains identical to the previous implementation.

use soroban_sdk::{contracttype, Address, BytesN, Env, Symbol, Vec};

use crate::keys;
use crate::types::{CallbackContext, CallbackStatus};
use crate::{EscrowError, DataKey};

/// Storage key namespace for callback nonces.
pub const CALLBACK_NONCE_KEY: Symbol = Symbol::short("cb_nonce");

/// Storage key namespace for callback contexts.
pub const CALLBACK_CONTEXT_KEY: Symbol = Symbol::short("cb_ctx");

/// Storage key namespace for callback consumption tracking.
pub const CALLBACK_CONSUMED_KEY: Symbol = Symbol::short("cb_used");

/// Returns the next callback nonce for the given escrow and increments the
/// stored counter. The nonce is scoped per escrow id.
pub fn next_callback_nonce(env: &Env, escrow_id: u64) -> u64 {
    let key = keys::callback_nonce(env, escrow_id);
    let current: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    let next = current.checked_add(1).unwrap_or(u64::MAX);
    env.storage().persistent().set(&key, &next);
    next
}

/// Reads the current callback nonce for the given escrow id without mutating.
pub fn get_callback_nonce(env: &Env, escrow_id: u64) -> u64 {
    let key = keys::callback_nonce(env, escrow_id);
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Stores a callback context under the escrow-scoped callback context key.
pub fn store_callback_context(env: &Env, escrow_id: u64, context: &CallbackContext) {
    let key = keys::callback_context(env, escrow_id);
    env.storage().persistent().set(&key, context);
}

/// Loads a callback context for the given escrow id, if present.
pub fn load_callback_context(env: &Env, escrow_id: u64) -> Option<CallbackContext> {
    let key = keys::callback_context(env, escrow_id);
    env.storage().persistent().get(&key)
}

/// Removes a callback context for the given escrow id.
pub fn remove_callback_context(env: &Env, escrow_id: u64) {
    let key = keys::callback_context(env, escrow_id);
    env.storage().persistent().remove(&key);
}

/// Marks a callback as consumed for the given escrow id and nonce.
pub fn mark_callback_consumed(env: &Env, escrow_id: u64, nonce: u64) {
    let key = keys::callback_consumed(env, escrow_id, nonce);
    env.storage().persistent().set(&key, &true);
}

/// Returns whether a callback has been consumed for the given escrow id and nonce.
pub fn is_callback_consumed(env: &Env, escrow_id: u64, nonce: u64) -> bool {
    let key = keys::callback_consumed(env, escrow_id, nonce);
    env.storage().persistent().get(&key).unwrap_or(false)
}

/// Internal helper: validates that a callback context is in a state that may
/// be executed. Returns the appropriate error otherwise.
fn ensure_executable(context: &CallbackContext) -> Result<(), EscrowError> {
    match context.status {
        CallbackStatus::Pending => Ok(()),
        CallbackStatus::Consumed => Err(EscrowError::CallbackAlreadyConsumed),
        CallbackStatus::Cancelled => Err(EscrowError::CallbackCancelled),
    }
}

/// Internal helper: builds a fresh callback context for the given escrow.
fn build_callback_context(
    env: &Env,
    escrow_id: u64,
    target: Address,
    payload: BytesN<32>,
    nonce: u64,
) -> CallbackContext {
    CallbackContext {
        escrow_id,
        target,
        payload,
        nonce,
        status: CallbackStatus::Pending,
        created_ledger: env.ledger().sequence(),
    }
}

/// Registers a callback for the given escrow id.
///
/// Returns the assigned callback nonce. Public signature is unchanged from the
/// previous implementation.
pub fn register_callback(
    env: Env,
    escrow_id: u64,
    target: Address,
    payload: BytesN<32>,
) -> Result<u64, EscrowError> {
    target.require_auth();

    if load_callback_context(&env, escrow_id).is_some() {
        return Err(EscrowError::CallbackAlreadyRegistered);
    }

    let nonce = next_callback_nonce(&env, escrow_id);
    let context = build_callback_context(&env, escrow_id, target, payload, nonce);
    store_callback_context(&env, escrow_id, &context);

    Ok(nonce)
}

/// Executes the registered callback for the given escrow id.
///
/// Returns the callback context that was executed. Public signature is
/// unchanged from the previous implementation.
pub fn execute_callback(env: Env, escrow_id: u64) -> Result<CallbackContext, EscrowError> {
    let mut context =
        load_callback_context(&env, escrow_id).ok_or(EscrowError::CallbackNotFound)?;

    ensure_executable(&context)?;

    if is_callback_consumed(&env, escrow_id, context.nonce) {
        return Err(EscrowError::CallbackAlreadyConsumed);
    }

    context.status = CallbackStatus::Consumed;
    store_callback_context(&env, escrow_id, &context);
    mark_callback_consumed(&env, escrow_id, context.nonce);

    Ok(context)
}

/// Returns the callback context for the given escrow id, if any.
///
/// Public signature is unchanged from the previous implementation.
pub fn get_callback(env: Env, escrow_id: u64) -> Option<CallbackContext> {
    load_callback_context(&env, escrow_id)
}

/// Returns whether the callback for the given escrow id has been consumed.
///
/// Public signature is unchanged from the previous implementation.
pub fn is_callback_consumed_for(env: Env, escrow_id: u64) -> bool {
    match load_callback_context(&env, escrow_id) {
        Some(context) => is_callback_consumed(&env, escrow_id, context.nonce),
        None => false,
    }
}

/// Returns the list of pending callback escrow ids tracked in storage.
///
/// Internal helper used by maintenance entrypoints.
pub fn pending_callback_ids(env: &Env) -> Vec<u64> {
    let key = DataKey::CallbackIndex;
    env.storage().persistent().get(&key).unwrap_or(Vec::new(env))
}

/// Adds an escrow id to the pending callback index.
pub fn index_pending_callback(env: &Env, escrow_id: u64) {
    let key = DataKey::CallbackIndex;
    let mut ids: Vec<u64> = env.storage().persistent().get(&key).unwrap_or(Vec::new(env));
    if !ids.contains(escrow_id) {
        ids.push_back(escrow_id);
        env.storage().persistent().set(&key, &ids);
    }
}

/// Removes an escrow id from the pending callback index.
pub fn unindex_pending_callback(env: &Env, escrow_id: u64) {
    let key = DataKey::CallbackIndex;
    let ids: Vec<u64> = env.storage().persistent().get(&key).unwrap_or(Vec::new(env));
    let mut filtered: Vec<u64> = Vec::new(env);
    for id in ids.iter() {
        if id != escrow_id {
            filtered.push_back(id);
        }
    }
    env.storage().persistent().set(&key, &filtered);
}