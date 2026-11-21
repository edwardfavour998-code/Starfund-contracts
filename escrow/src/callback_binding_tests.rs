//! Unit and integration tests for cross-contract callback binding (Issue #1222).
//!
//! Verifies:
//! - Registration of callbacks with invocation nonce, origin address, and expected phase.
//! - Execution and single-use consumption of callbacks.
//! - Guard against wrong origin (replaying against a different contract/origin).
//! - Guard against wrong nonce (mismatched invocation sequence).
//! - Guard against callback replay (double execution of the same nonce).
//! - Guard against callback execution after escrow cancellation.
//! - Guard against wrong phase.
//! - Auth validation for admin (registration) and origin (execution).

use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    Address, Env, IntoVal, String, Symbol, TryIntoVal,
};

use crate::callback::CallbackContext;
use crate::tests::assert_contract_error;

use super::{EscrowError, StarfundEscrow, StarfundEscrowClient};

/// Deploy and initialize an escrow instance for callback testing.
fn deploy_escrow<'a>(
    env: &'a Env,
    invoice_id: &str,
) -> (StarfundEscrowClient<'a>, Address, Address, Address) {
    env.mock_all_auths_allowing_non_root_auth();
    let id = env.register(StarfundEscrow, ());
    let client = StarfundEscrowClient::new(env, &id);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let token = Address::generate(env);
    let treasury = Address::generate(env);
    client.init(
        &admin,
        &String::from_str(env, invoice_id),
        &sme,
        &1_000i128,
        &800i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None);
    (client, admin, sme, id)
}

// -----------------------------------------------------------------------------
// Edge case 1: Valid callback
// -----------------------------------------------------------------------------

#[test]
fn test_valid_callback_registration_and_execution() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_1");

    let origin = Address::generate(&env);
    let phase = 1u32;

    assert_eq!(client.get_callback_nonce(), 0);
    assert_eq!(client.get_callback(&1), None);
    assert!(!client.is_callback_consumed(&1));

    // Step 1: Register callback
    let nonce = client.register_callback(&origin, &phase);
    assert_eq!(nonce, 1);
    assert_eq!(client.get_callback_nonce(), 1);

    let stored = client.get_callback(&nonce).expect("callback should exist");
    assert_eq!(
        stored,
        CallbackContext {
            origin: origin.clone(),
            nonce: 1,
            phase: 1,
            created_at: env.ledger().timestamp(),
            consumed: false,
        }
    );
    assert!(!client.is_callback_consumed(&nonce));

    // Step 2: Execute callback
    let executed = client.execute_callback(&nonce, &origin, &phase);
    assert_eq!(executed.nonce, 1);
    assert_eq!(executed.origin, origin);
    assert_eq!(executed.phase, phase);
    assert!(executed.consumed);
    assert!(client.is_callback_consumed(&nonce));

    let after = client.get_callback(&nonce).expect("callback should exist");
    assert!(after.consumed);
}

// -----------------------------------------------------------------------------
// Edge case 2: Wrong origin
// -----------------------------------------------------------------------------

#[test]
fn test_callback_wrong_origin_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_2");

    let origin_a = Address::generate(&env);
    let origin_b = Address::generate(&env);
    let phase = 1u32;

    let nonce = client.register_callback(&origin_a, &phase);
    assert_eq!(nonce, 1);

    // Attempt callback execution with wrong origin (origin_b instead of origin_a)
    assert_contract_error(
        client.try_execute_callback(&nonce, &origin_b, &phase),
        EscrowError::CallbackWrongOrigin,
    );

    // Ensure state was not corrupted / consumed
    assert!(!client.is_callback_consumed(&nonce));
    let stored = client
        .get_callback(&nonce)
        .expect("callback should still exist");
    assert!(!stored.consumed);
}

// -----------------------------------------------------------------------------
// Edge case 3: Wrong nonce
// -----------------------------------------------------------------------------

#[test]
fn test_callback_wrong_nonce_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_3");

    let origin = Address::generate(&env);
    let phase = 1u32;

    let nonce = client.register_callback(&origin, &phase);
    assert_eq!(nonce, 1);

    // Nonce that does not exist
    assert_contract_error(
        client.try_execute_callback(&999u64, &origin, &phase),
        EscrowError::CallbackNotFound,
    );

    // Context for nonce 1 is still intact and unconsumed
    assert!(!client.is_callback_consumed(&1));
}

#[test]
fn test_callback_stored_nonce_mismatch_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, id) = deploy_escrow(&env, "INV_CB_3C");

    let origin = Address::generate(&env);
    let phase = 1u32;
    let nonce = client.register_callback(&origin, &phase);
    let mut context = client.get_callback(&nonce).expect("callback should exist");
    context.nonce = nonce + 1;

    env.as_contract(&id, || {
        env.storage()
            .instance()
            .set(&crate::callback::DataKey::CallbackContext(nonce), &context);
    });

    assert_contract_error(
        client.try_execute_callback(&nonce, &origin, &phase),
        EscrowError::CallbackWrongNonce,
    );
}

#[test]
fn test_callback_mismatched_nonce_between_origins() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_3B");

    let origin_a = Address::generate(&env);
    let origin_b = Address::generate(&env);

    let nonce_1 = client.register_callback(&origin_a, &1u32);
    let nonce_2 = client.register_callback(&origin_b, &2u32);

    assert_eq!(nonce_1, 1);
    assert_eq!(nonce_2, 2);

    // Origin A attempts to consume Nonce 2 (registered for Origin B)
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.execute_callback(&nonce_2, &origin_a, &1u32);
    }));
    assert!(
        res.is_err(),
        "origin A must not be able to execute callback registered for origin B"
    );

    assert!(!client.is_callback_consumed(&nonce_1));
    assert!(!client.is_callback_consumed(&nonce_2));
}

// -----------------------------------------------------------------------------
// Edge case 4: Callback replay
// -----------------------------------------------------------------------------

#[test]
fn test_callback_replay_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_4");

    let origin = Address::generate(&env);
    let phase = 1u32;

    let nonce = client.register_callback(&origin, &phase);

    // First execution succeeds
    let first = client.execute_callback(&nonce, &origin, &phase);
    assert!(first.consumed);
    assert!(client.is_callback_consumed(&nonce));

    // Second execution (replay) must fail
    assert_contract_error(
        client.try_execute_callback(&nonce, &origin, &phase),
        EscrowError::CallbackReplayed,
    );

    // Ensure status remains consumed
    assert!(client.is_callback_consumed(&nonce));
}

// -----------------------------------------------------------------------------
// Edge case 5: Callback after cancellation / terminal state
// -----------------------------------------------------------------------------

#[test]
fn test_callback_after_cancellation_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_5");

    let origin = Address::generate(&env);
    let phase = 1u32;

    // Register callback while open (status == 0)
    let nonce = client.register_callback(&origin, &phase);
    assert_eq!(nonce, 1);

    // Cancel funding (transitions status to 4)
    client.cancel_funding(&0u32);
    assert_eq!(client.get_escrow().status, 4);
    assert_eq!(client.get_callback(&nonce), None);

    // Attempt callback execution after cancellation — still uses CallbackAfterCancellation
    assert_contract_error(
        client.try_execute_callback(&nonce, &origin, &phase),
        EscrowError::CallbackAfterCancellation,
    );

    // Attempt registering a new callback on cancelled escrow must fail with
    // CallbackOnTerminalEscrow (covers status 4 via the active-states guard).
    assert_contract_error(
        client.try_register_callback(&origin, &phase),
        EscrowError::CallbackOnTerminalEscrow,
    );
}

/// register_callback must reject status 2 (Settled) with CallbackOnTerminalEscrow.
#[test]
fn test_register_callback_on_settled_escrow_rejected() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    // Use a real token so we can fund and settle.
    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token_id = sac.address();
    let sac_admin = soroban_sdk::token::StellarAssetClient::new(&env, &token_id);

    let escrow_id = env.register(StarfundEscrow, ());
    let client = StarfundEscrowClient::new(&env, &escrow_id);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let investor = Address::generate(&env);
    let target = 1_000i128;

    client.init(
        &admin,
        &String::from_str(&env, "INV_CB_S2"),
        &sme,
        &target,
        &800i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None::<i64>,
        &None::<u32>,
    );

    sac_admin.mint(&investor, &target);
    client.fund(&investor, &target);
    assert_eq!(client.get_escrow().status, 1);

    // Settle: inject enough tokens and settle (maturity=0 is already past)
    sac_admin.mint(&escrow_id, &target);
    client.settle();
    assert_eq!(client.get_escrow().status, 2);

    let origin = Address::generate(&env);
    assert_contract_error(
        client.try_register_callback(&origin, &1u32),
        EscrowError::CallbackOnTerminalEscrow,
    );
}

/// register_callback must reject status 3 (Withdrawn) with CallbackOnTerminalEscrow.
#[test]
fn test_register_callback_on_withdrawn_escrow_rejected() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let sac = env.register_stellar_asset_contract_v2(Address::generate(&env));
    let token_id = sac.address();
    let sac_admin = soroban_sdk::token::StellarAssetClient::new(&env, &token_id);

    let escrow_id = env.register(StarfundEscrow, ());
    let client = StarfundEscrowClient::new(&env, &escrow_id);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let investor = Address::generate(&env);
    let target = 1_000i128;

    client.init(
        &admin,
        &String::from_str(&env, "INV_CB_S3"),
        &sme,
        &target,
        &800i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None::<i64>,
        &None::<u32>,
    );

    sac_admin.mint(&investor, &target);
    client.fund(&investor, &target);
    assert_eq!(client.get_escrow().status, 1);

    // Mint tokens into escrow and withdraw (status 1 → 3)
    sac_admin.mint(&escrow_id, &target);
    client.withdraw();
    assert_eq!(client.get_escrow().status, 3);

    let origin = Address::generate(&env);
    assert_contract_error(
        client.try_register_callback(&origin, &1u32),
        EscrowError::CallbackOnTerminalEscrow,
    );
}

// -----------------------------------------------------------------------------
// Additional edge cases: Wrong phase and multi-callback sequentiality
// -----------------------------------------------------------------------------

#[test]
fn test_callback_wrong_phase_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_PHASE");

    let origin = Address::generate(&env);
    let expected_phase = 1u32;
    let wrong_phase = 2u32;

    let nonce = client.register_callback(&origin, &expected_phase);

    assert_contract_error(
        client.try_execute_callback(&nonce, &origin, &wrong_phase),
        EscrowError::CallbackWrongPhase,
    );

    assert!(!client.is_callback_consumed(&nonce));
}

#[test]
fn test_multiple_callbacks_independent_consumption() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_MULTI");

    let origin_a = Address::generate(&env);
    let origin_b = Address::generate(&env);

    let nonce_1 = client.register_callback(&origin_a, &10u32);
    let nonce_2 = client.register_callback(&origin_b, &20u32);
    let nonce_3 = client.register_callback(&origin_a, &30u32);

    assert_eq!(nonce_1, 1);
    assert_eq!(nonce_2, 2);
    assert_eq!(nonce_3, 3);
    assert_eq!(client.get_callback_nonce(), 3);

    // Consume out of order: consume #2 first
    client.execute_callback(&nonce_2, &origin_b, &20u32);
    assert!(!client.is_callback_consumed(&1));
    assert!(client.is_callback_consumed(&2));
    assert!(!client.is_callback_consumed(&3));

    // Consume #1 next
    client.execute_callback(&nonce_1, &origin_a, &10u32);
    assert!(client.is_callback_consumed(&1));
    assert!(client.is_callback_consumed(&2));
    assert!(!client.is_callback_consumed(&3));

    // Consume #3 last
    client.execute_callback(&nonce_3, &origin_a, &30u32);
    assert!(client.is_callback_consumed(&1));
    assert!(client.is_callback_consumed(&2));
    assert!(client.is_callback_consumed(&3));
}

// -----------------------------------------------------------------------------
// Issue #378: explicit cancellation of obsolete pending callbacks
// -----------------------------------------------------------------------------

/// Admin-authorized cancellation removes the pending index entry and emits the
/// cancellation event.
#[test]
fn test_cancel_callback_authorized_removes_entry_and_emits_event() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_CANCEL_OK");

    let origin = Address::generate(&env);
    let phase = 7u32;
    let nonce = client.register_callback(&origin, &phase);
    assert!(client.get_callback(&nonce).is_some());

    client.cancel_callback(&nonce);

    // Pending index entry is gone.
    assert_eq!(client.get_callback(&nonce), None);
    assert!(!client.is_callback_consumed(&nonce));

    // Cancellation event was emitted.
    let events = env.events().all();
    let mut found = false;
    for (contract, topics, data) in events.iter() {
        if contract != client.address {
            continue;
        }
        let topic0: Symbol = topics
            .get(0)
            .expect("event topic")
            .try_into_val(&env)
            .expect("symbol topic");
        if topic0 == Symbol::new(&env, "callback_cancelled") {
            let emitted_nonce: u64 = data.try_into_val(&env).expect("nonce payload");
            assert_eq!(emitted_nonce, nonce);
            found = true;
        }
    }
    assert!(found, "callback_cancelled event must be emitted");
}

/// Only the admin may cancel a pending callback.
#[test]
fn test_cancel_callback_unauthorized_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_CANCEL_UNAUTH");

    let origin = Address::generate(&env);
    let nonce = client.register_callback(&origin, &1u32);

    // Drop the blanket auth mocking so the admin auth check is enforced.
    env.set_auths(&[]);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.cancel_callback(&nonce);
    }));
    assert!(res.is_err(), "non-admin cancellation must be rejected");

    // Entry remains pending after the rejected attempt.
    assert!(client.get_callback(&nonce).is_some());
}

/// Cancelling one callback leaves other registrations intact and executable.
#[test]
fn test_cancel_callback_leaves_other_callbacks_intact() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_CANCEL_OTHER");

    let origin_a = Address::generate(&env);
    let origin_b = Address::generate(&env);
    let nonce_1 = client.register_callback(&origin_a, &1u32);
    let nonce_2 = client.register_callback(&origin_b, &2u32);

    client.cancel_callback(&nonce_1);
    assert_eq!(client.get_callback(&nonce_1), None);
    assert!(client.get_callback(&nonce_2).is_some());

    // The surviving callback still executes normally.
    let executed = client.execute_callback(&nonce_2, &origin_b, &2u32);
    assert_eq!(executed.nonce, nonce_2);
    assert!(client.is_callback_consumed(&nonce_2));
}

/// A cancelled callback can never be executed afterwards.
#[test]
fn test_cancelled_callback_cannot_execute() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_CANCEL_EXEC");

    let origin = Address::generate(&env);
    let phase = 1u32;
    let nonce = client.register_callback(&origin, &phase);

    client.cancel_callback(&nonce);

    assert_contract_error(
        client.try_execute_callback(&nonce, &origin, &phase),
        EscrowError::CallbackCancelled,
    );
    assert!(!client.is_callback_consumed(&nonce));
}

/// Cancelling an unknown nonce is rejected.
#[test]
fn test_cancel_unknown_callback_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_CANCEL_UNKNOWN");

    assert_contract_error(
        client.try_cancel_callback(&999u64),
        EscrowError::CallbackNotFound,
    );
}

// -----------------------------------------------------------------------------
// Issue #379: reject escrow self-address as callback origin
// -----------------------------------------------------------------------------

/// Registering with the escrow's own address fails before nonce allocation.
#[test]
fn test_register_callback_self_origin_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_SELF");

    let self_address = client.address.clone();
    assert_contract_error(
        client.try_register_callback(&self_address, &1u32),
        EscrowError::CallbackInvalidOrigin,
    );

    // No nonce was allocated and no entry was stored.
    assert_eq!(client.get_callback_nonce(), 0);
    assert_eq!(client.get_callback(&1), None);
}

/// A real external origin still registers after the self-origin guard.
#[test]
fn test_register_callback_external_origin_still_supported() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_EXT");

    let origin = Address::generate(&env);
    let nonce = client.register_callback(&origin, &1u32);
    assert_eq!(nonce, 1);
    assert_eq!(client.get_callback_nonce(), 1);
    assert!(client.get_callback(&nonce).is_some());
}

// -----------------------------------------------------------------------------
// Issue #380: callback age / retirement policy
// -----------------------------------------------------------------------------

/// Documented policy: pending callbacks remain valid indefinitely unless
/// explicitly cancelled by the admin. An old pending callback still executes.
#[test]
fn test_old_pending_callback_remains_valid_indefinitely() {
    let env = Env::default();
    let (client, _admin, _sme, _id) = deploy_escrow(&env, "INV_CB_AGE");

    let origin = Address::generate(&env);
    let phase = 1u32;
    let nonce = client.register_callback(&origin, &phase);
    let created_at = client
        .get_callback(&nonce)
        .expect("callback should exist")
        .created_at;

    // Advance the ledger far into the future (well past any plausible expiry).
    env.ledger().set_timestamp(created_at + 10 * 365 * 24 * 60 * 60);

    // No implicit expiry: the callback is still pending and executable.
    assert!(client.get_callback(&nonce).is_some());
    let executed = client.execute_callback(&nonce, &origin, &phase);
    assert_eq!(executed.nonce, nonce);
    assert!(client.is_callback_consumed(&nonce));
}
