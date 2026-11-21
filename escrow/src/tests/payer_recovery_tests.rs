use crate::{
    tests::{assert_contract_error, deploy, free_addresses, setup},
    EscrowError, PayerRecoveryAccepted, PayerRecoveryProposed, PendingPayerCancelled,
};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events, Ledger as _},
    Address, Env, Event, String,
};

#[test]
fn test_propose_payer_recovery_admin_gated() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_001"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    let result = client.propose_payer_recovery(&new_payer, &None);
    assert_eq!(result, new_payer);

    let escrow = client.get_escrow();
    // Current payer should not have changed yet
    assert_eq!(escrow.payer, admin);
}

#[test]
fn test_propose_payer_recovery_non_admin_fails() {
    let env = Env::default();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    // Restrict authorization to the admin only so init succeeds but any
    // subsequent non-admin authorization is rejected.
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &admin,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &client.address,
            fn_name: "init",
            args: soroban_sdk::IntoVal::into_val(
                &(
                    admin.clone(),
                    String::from_str(&env, "PAYER_REC_002"),
                    sme.clone(),
                    100_000_000_000i128,
                    500i64,
                    0u64,
                    token.clone(),
                    Option::<Address>::None,
                    treasury.clone(),
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                    Option::<Address>::None,
                ),
                &env,
            ),
            sub_invokes: &[],
        },
    }]);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_002"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let escrow_before = client.get_escrow();
    assert_eq!(escrow_before.payer, admin);
    assert_eq!(escrow_before.pending_payer, None);

    // Clear all authorizations: the non-admin propose must fail on auth.
    env.mock_auths(&[]);
    let new_payer = Address::generate(&env);
    let res = client.try_propose_payer_recovery(&new_payer, &None);
    match res {
        Err(Ok(err)) => assert_eq!(
            err,
            soroban_sdk::InvokeError::from(soroban_sdk::xdr::ScError::Context(
                soroban_sdk::xdr::ScErrorType::Auth,
                soroban_sdk::xdr::ScErrorCode::InvalidAction,
            )),
            "expected authorization failure, got {:?}",
            err
        ),
        other => panic!("expected authorization failure, got {:?}", other),
    }

    // State must be unchanged after the rejected call.
    let escrow_after = client.get_escrow();
    assert_eq!(escrow_after.payer, admin);
    assert_eq!(escrow_after.pending_payer, None);
}

#[test]
fn test_propose_payer_recovery_same_payer_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_003"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    // Try to propose the current payer (admin) as the new payer
    assert_contract_error(
        client.try_propose_payer_recovery(&admin, &None),
        EscrowError::NewPayerSameAsCurrent,
    );
}

#[test]
fn test_propose_payer_recovery_self_address_fails_before_writes() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_014"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    // Propose the current payer as the new payer: must be rejected.
    assert_contract_error(
        client.try_propose_payer_recovery(&admin, &None),
        EscrowError::NewPayerSameAsCurrent,
    );

    // No writes: payer unchanged and no pending proposal recorded.
    let escrow = client.get_escrow();
    assert_eq!(escrow.payer, admin);
    assert_contract_error(
        client.try_accept_payer_recovery(),
        EscrowError::NoPendingPayer,
    );
}

#[test]
fn test_propose_payer_recovery_valid_new_payer_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_015"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    // A distinct valid address is accepted as a recovery nomination.
    let new_payer = Address::generate(&env);
    assert_ne!(new_payer, admin);
    let result = client.propose_payer_recovery(&new_payer, &None);
    assert_eq!(result, new_payer);

    // Payer is still unchanged until acceptance.
    let escrow = client.get_escrow();
    assert_eq!(escrow.payer, admin);

    // The proposal can be accepted and updates the payer.
    let updated_escrow = client.accept_payer_recovery();
    assert_eq!(updated_escrow.payer, new_payer);
}

#[test]
fn test_propose_payer_recovery_not_open_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_004"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    // Cancel the escrow to move it out of open/funded status
    client.cancel_funding(&0u32);

    let new_payer = Address::generate(&env);
    assert_contract_error(
        client.try_propose_payer_recovery(&new_payer, &None),
        EscrowError::PayerRecoveryNotOpen,
    );
}

#[test]
fn test_accept_payer_recovery_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_005"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);

    let updated_escrow = client.accept_payer_recovery();
    assert_eq!(updated_escrow.payer, new_payer);
}

#[test]
fn test_accept_payer_recovery_new_payer_auth_required() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_006"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);

    // Attempt accept without new_payer auth should fail.
    env.mock_auths(&[]);
    let res = client.try_accept_payer_recovery();
    assert!(res.is_err());
}

#[test]
fn test_accept_payer_recovery_no_pending_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_007"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    // Try to accept without proposing first
    assert_contract_error(
        client.try_accept_payer_recovery(),
        EscrowError::NoPendingPayer,
    );
}

#[test]
fn test_accept_payer_recovery_expired_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_008"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    // Propose with a very short validity window (1 second)
    client.propose_payer_recovery(&new_payer, &Some(1));

    // Advance ledger time past expiry
    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp += 2;
    env.ledger().set(ledger_info);

    assert_contract_error(
        client.try_accept_payer_recovery(),
        EscrowError::PayerProposalExpired,
    );
}

#[test]
fn test_cancel_pending_payer_admin_gated() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_009"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);

    // Admin can cancel
    client.cancel_pending_payer();

    // Verify payer hasn't changed
    let escrow = client.get_escrow();
    assert_eq!(escrow.payer, admin);
}

#[test]
fn test_cancel_pending_payer_non_admin_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_010"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);

    // Non-admin try to cancel
    let non_admin = Address::generate(&env);
    env.mock_all_auths();
    let res = client.try_cancel_pending_payer();
    assert!(res.is_err());
}

#[test]
fn test_cancel_pending_payer_no_pending_fails() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_011"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    // Try to cancel without proposing first
    assert_contract_error(
        client.try_cancel_pending_payer(),
        EscrowError::NoPendingPayer,
    );
}

#[test]
fn test_payer_recovery_emits_correct_events() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);
    let contract_id = client.address.clone();

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_012"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);

    let invoice_id = soroban_sdk::Symbol::new(&env, "PAYER_REC_012");
    let proposal = PayerRecoveryProposed {
        name: symbol_short!("payer_prp"),
        invoice_id: invoice_id.clone(),
        current_payer: admin.clone(),
        pending_payer: new_payer.clone(),
    }
    .to_xdr(&env, &contract_id);
    let all_events = env.events().all();
    let events = all_events.events();
    assert!(events.contains(&proposal), "PayerRecoveryProposed event not found");

    // Accept and verify accepted event
    client.accept_payer_recovery();
    let all_events_after = env.events().all();
    let accept_event = PayerRecoveryAccepted {
        name: symbol_short!("payer_acc"),
        invoice_id: invoice_id.clone(),
        prior_payer: admin.clone(),
        new_payer: new_payer.clone(),
    }
    .to_xdr(&env, &contract_id);
    let after_events = all_events_after.events();
    assert!(after_events.contains(&accept_event), "PayerRecoveryAccepted event not found");
}

#[test]
fn test_payer_recovery_view_absent_returns_none() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_014"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    assert!(client.get_pending_payer_recovery().is_none());
}

#[test]
fn test_payer_recovery_view_matches_storage_after_propose() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_015"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &Some(3600));

    let view = client
        .get_pending_payer_recovery()
        .expect("pending payer recovery view should be present");
    assert_eq!(view.pending_payer, new_payer);
    assert_eq!(view.proposed_at, env.ledger().timestamp());
    assert_eq!(view.expires_at, env.ledger().timestamp() + 3600);
}

#[test]
fn test_payer_recovery_view_updates_on_replacement() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_016"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let first_payer = Address::generate(&env);
    client.propose_payer_recovery(&first_payer, &Some(100));
    let first_view = client.get_pending_payer_recovery().unwrap();
    assert_eq!(first_view.pending_payer, first_payer);

    // Advance time so the replacement has a distinct proposal timestamp.
    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp += 10;
    env.ledger().set(ledger_info);

    let second_payer = Address::generate(&env);
    client.propose_payer_recovery(&second_payer, &Some(200));

    let second_view = client.get_pending_payer_recovery().unwrap();
    assert_eq!(second_view.pending_payer, second_payer);
    assert_eq!(second_view.proposed_at, env.ledger().timestamp());
    assert_eq!(second_view.expires_at, env.ledger().timestamp() + 200);
}

#[test]
fn test_payer_recovery_view_cleared_on_accept() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_017"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);
    assert!(client.get_pending_payer_recovery().is_some());

    let updated = client.accept_payer_recovery();
    assert_eq!(updated.payer, new_payer);
    assert!(client.get_pending_payer_recovery().is_none());
}

#[test]
fn test_payer_recovery_view_cleared_on_cancel() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_018"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);
    assert!(client.get_pending_payer_recovery().is_some());

    client.cancel_pending_payer();
    assert!(client.get_pending_payer_recovery().is_none());
    assert_eq!(client.get_escrow().payer, admin);
}

#[test]
fn test_payer_recovery_full_lifecycle() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, sme) = setup(&env);

    let (token, treasury) = free_addresses(&env);
    client.init(
        &admin,
        &String::from_str(&env, "PAYER_REC_013"),
        &sme,
        &100_000_000_000i128,
        &500i64,
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

    let initial_payer = admin.clone();
    let escrow_before = client.get_escrow();
    assert_eq!(escrow_before.payer, initial_payer);

    // Propose recovery
    let new_payer = Address::generate(&env);
    client.propose_payer_recovery(&new_payer, &None);

    // Accept recovery
    let escrow_after = client.accept_payer_recovery();
    assert_eq!(escrow_after.payer, new_payer);
    assert_ne!(escrow_after.payer, initial_payer);
}
