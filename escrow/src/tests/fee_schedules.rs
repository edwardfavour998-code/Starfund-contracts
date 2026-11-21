use super::{decode_events, setup, TestConfig};
use crate::{FeeSchedule, FeeScheduleActivated, FeeScheduleSubmitted, StarfundEscrowClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Ledger as _},
    Address, Env,
};

fn init_fee_schedule_test<'a>(env: &'a Env) -> (StarfundEscrowClient<'a>, Address) {
    let (client, admin, sme) = setup(env);
    TestConfig::new(env, "FEES01", &sme)
        .amount(100_000i128)
        .maturity(800i64)
        .init(&client, &admin);
    (client, admin)
}

#[test]
fn fee_schedule_views_project_pending_activation_without_promotion() {
    let env = Env::default();
    let (client, _admin) = init_fee_schedule_test(&env);
    let contract_id = client.address.clone();

    let previous = FeeSchedule {
        fee_bps: 100,
        min_fee_bps: 50,
        max_fee_bps: 150,
        activation_ledger: 110,
    };
    client.submit_fee_schedule(&previous);
    let mut ledger = env.ledger().get();
    ledger.sequence_number = previous.activation_ledger;
    env.ledger().set(ledger);
    assert!(client.activate_fee_schedule());

    let pending = FeeSchedule {
        fee_bps: 200,
        min_fee_bps: 100,
        max_fee_bps: 250,
        activation_ledger: 130,
    };
    client.submit_fee_schedule(&pending);

    let storage_before = env.storage().get_contract_storage_snapshot(&contract_id);
    let events_before = env.events().all().events().len();

    // Immediately before activation: pending is still projected as pending.
    let mut ledger = env.ledger().get();
    ledger.sequence_number = pending.activation_ledger - 1;
    ledger.timestamp = 1_000;
    env.ledger().set(ledger);
    let active_before = client.get_active_fee_schedule();
    let pending_before = client.get_pending_fee_schedule();
    let previous_before = client.get_previous_fee_schedule();
    assert_eq!(active_before, Some(previous.clone()));
    assert_eq!(pending_before, Some(pending.clone()));
    assert_eq!(previous_before, None);

    // At activation ledger: projection promotes the pending schedule.
    let mut ledger = env.ledger().get();
    ledger.sequence_number = pending.activation_ledger;
    ledger.timestamp = 2_000;
    env.ledger().set(ledger);
    let active_at = client.get_active_fee_schedule();
    let pending_at = client.get_pending_fee_schedule();
    let previous_at = client.get_previous_fee_schedule();
    assert_eq!(active_at, Some(pending.clone()));
    assert_eq!(pending_at, None);
    assert_eq!(previous_at, Some(previous.clone()));

    // After activation ledger: projection stays promoted.
    let mut ledger = env.ledger().get();
    ledger.sequence_number = pending.activation_ledger + 1;
    ledger.timestamp = 3_000;
    env.ledger().set(ledger);
    let active_after = client.get_active_fee_schedule();
    let pending_after = client.get_pending_fee_schedule();
    let previous_after = client.get_previous_fee_schedule();
    assert_eq!(active_after, Some(pending.clone()));
    assert_eq!(pending_after, None);
    assert_eq!(previous_after, Some(previous.clone()));

    // Reads must not mutate storage or emit events.
    assert_eq!(
        env.storage().get_contract_storage_snapshot(&contract_id),
        storage_before
    );
    assert_eq!(env.events().all().events().len(), events_before);

    // Persisted promotion matches the projected views.
    assert!(client.activate_fee_schedule());
    assert_eq!(client.get_active_fee_schedule(), active_after);
    assert_eq!(client.get_pending_fee_schedule(), pending_after);
    assert_eq!(client.get_previous_fee_schedule(), previous_after);
    assert_eq!(
        env.events().all().events().last().unwrap().clone(),
        FeeScheduleActivated {
            name: symbol_short!("fee_act"),
            invoice_id: client.get_escrow().invoice_id,
            schedule: pending,
            previous: Some(previous),
        }
        .to_xdr(&env, &contract_id)
    );
}

#[test]
fn fee_schedule_changes_emit_events() {
    let env = Env::default();
    let (client, _admin) = init_fee_schedule_test(&env);
    let contract_id = client.address.clone();
    let first = FeeSchedule {
        fee_bps: 100,
        min_fee_bps: 50,
        max_fee_bps: 150,
        activation_ledger: 110,
    };

    client.submit_fee_schedule(&first);
    let submitted = decode_events::<FeeScheduleSubmitted>(
        &env,
        &contract_id,
        symbol_short!("fee_sub"),
    );
    assert_eq!(submitted.len(), 1);
    assert_eq!(
        submitted[0],
        FeeScheduleSubmitted {
            name: symbol_short!("fee_sub"),
            invoice_id: client.get_escrow().invoice_id,
            schedule: first.clone(),
        }
    );

    let event_count = env.events().all().events().len();
    client.submit_fee_schedule(&first);
    assert_eq!(env.events().all().events().len(), event_count);

    let mut ledger = env.ledger().get();
    ledger.sequence_number = first.activation_ledger;
    env.ledger().set(ledger);
    assert!(client.activate_fee_schedule());
    let activated = decode_events::<FeeScheduleActivated>(
        &env,
        &contract_id,
        symbol_short!("fee_act"),
    );
    assert_eq!(activated.len(), 1);
    assert_eq!(
        activated[0],
        FeeScheduleActivated {
            name: symbol_short!("fee_act"),
            invoice_id: client.get_escrow().invoice_id,
            schedule: first.clone(),
            previous: None,
        }
    );

    let second = FeeSchedule {
        fee_bps: 200,
        min_fee_bps: 100,
        max_fee_bps: 250,
        activation_ledger: 120,
    };
    client.submit_fee_schedule(&second);
    let mut ledger = env.ledger().get();
    ledger.sequence_number = second.activation_ledger;
    env.ledger().set(ledger);
    assert!(client.activate_fee_schedule());
    let activated = decode_events::<FeeScheduleActivated>(
        &env,
        &contract_id,
        symbol_short!("fee_act"),
    );
    assert_eq!(activated.len(), 2);
    assert_eq!(
        activated[1],
        FeeScheduleActivated {
            name: symbol_short!("fee_act"),
            invoice_id: client.get_escrow().invoice_id,
            schedule: second,
            previous: Some(first),
        }
    );
}

/// Reference implementation of the approved fee rounding policy (issue #224):
/// fee = floor(gross * fee_bps / 10_000), net = gross - fee.
fn expected_fee(gross: i128, fee_bps: i64) -> i128 {
    gross * (fee_bps as i128) / 10_000
}

fn init_fee_split_test<'a>(
    env: &'a Env,
    amount: i128,
    fee_bps: i64,
) -> (StarfundEscrowClient<'a>, Address, Address, Address) {
    let (client, admin, sme) = setup(env);
    let (funding_token, treasury) = free_addresses(env);
    client.init(
        &admin,
        &String::from_str(env, "FEESPLIT"),
        &sme,
        &amount,
        &fee_bps,
        &0u64,
        &funding_token,
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
    (client, admin, sme, treasury)
}

fn assert_gross_conserved(gross: i128, fee: i128, net: i128) {
    assert_eq!(fee + net, gross, "gross must equal fee plus net");
    assert!(fee >= 0, "fee must be non-negative");
    assert!(net >= 0, "net must be non-negative");
}

#[test]
fn release_fee_boundaries_conserve_gross() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 10_000i128;
    let (client, _admin, sme, treasury) = init_fee_split_test(&env, amount, 250);

    let fee = expected_fee(amount, 250);
    let net = amount - fee;
    assert_gross_conserved(amount, fee, net);

    client.release(&sme);
    assert_eq!(client.get_escrow().released, amount);
    assert_eq!(client.get_escrow().fee_amount, fee);
    assert_eq!(client.get_escrow().net_amount, net);
}

#[test]
fn release_fee_rounds_down_for_odd_amounts() {
    let env = Env::default();
    env.mock_all_auths();
    // 333 * 250 / 10_000 = 8.325 -> floor 8 (rounding direction preserved).
    let amount = 333i128;
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, 250);

    let fee = expected_fee(amount, 250);
    assert_eq!(fee, 8);
    let net = amount - fee;
    assert_gross_conserved(amount, fee, net);

    client.release(&sme);
    assert_eq!(client.get_escrow().fee_amount, fee);
    assert_eq!(client.get_escrow().net_amount, net);
}

#[test]
fn release_zero_fee_rate_conserves_gross() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 5_000i128;
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, 0);

    client.release(&sme);
    assert_eq!(client.get_escrow().fee_amount, 0);
    assert_eq!(client.get_escrow().net_amount, amount);
    assert_gross_conserved(amount, 0, amount);
}

#[test]
fn release_rejects_invalid_fee_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, 1_000i128, 10_001);

    let result = client.try_release(&sme);
    assert_eq!(result, Err(Ok(EscrowError::InvalidFeeRate)));
}

#[test]
fn release_rejects_fee_rate_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, i128::MAX, 10_000);

    let result = client.try_release(&sme);
    assert_eq!(result, Err(Ok(EscrowError::FeeOverflow)));
}

#[test]
fn withdraw_fee_boundaries_conserve_gross() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 10_000i128;
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, 250);

    let fee = expected_fee(amount, 250);
    let net = amount - fee;
    assert_gross_conserved(amount, fee, net);

    client.withdraw(&sme);
    assert_eq!(client.get_escrow().fee_amount, fee);
    assert_eq!(client.get_escrow().net_amount, net);
}

#[test]
fn withdraw_fee_rounds_down_for_odd_amounts() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 333i128;
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, 250);

    let fee = expected_fee(amount, 250);
    assert_eq!(fee, 8);
    let net = amount - fee;
    assert_gross_conserved(amount, fee, net);

    client.withdraw(&sme);
    assert_eq!(client.get_escrow().fee_amount, fee);
    assert_eq!(client.get_escrow().net_amount, net);
}

#[test]
fn withdraw_rejects_invalid_fee_rate() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, 1_000i128, 10_001);

    let result = client.try_withdraw(&sme);
    assert_eq!(result, Err(Ok(EscrowError::InvalidFeeRate)));
}

#[test]
fn withdraw_rejects_fee_rate_overflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, i128::MAX, 10_000);

    let result = client.try_withdraw(&sme);
    assert_eq!(result, Err(Ok(EscrowError::FeeOverflow)));
}

#[test]
fn release_and_withdraw_share_fee_arithmetic() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 7_777i128;
    let fee_bps = 375i64;

    let (release_client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, fee_bps);
    release_client.release(&sme);
    let release_fee = release_client.get_escrow().fee_amount;
    let release_net = release_client.get_escrow().net_amount;

    let env2 = Env::default();
    env2.mock_all_auths();
    let (withdraw_client, _admin, sme, _treasury) = init_fee_split_test(&env2, amount, fee_bps);
    withdraw_client.withdraw(&sme);
    let withdraw_fee = withdraw_client.get_escrow().fee_amount;
    let withdraw_net = withdraw_client.get_escrow().net_amount;

    assert_eq!(release_fee, withdraw_fee);
    assert_eq!(release_net, withdraw_net);
    assert_eq!(release_fee, expected_fee(amount, fee_bps));
    assert_gross_conserved(amount, release_fee, release_net);
}

#[test]
fn cumulative_partial_releases_match_approved_rounding() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 1_000i128;
    let fee_bps = 250i64;
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, fee_bps);

    let tranches = [333i128, 333i128, 334i128];
    let mut cumulative_fee = 0i128;
    let mut cumulative_net = 0i128;
    let mut cumulative_gross = 0i128;

    for tranche in tranches {
        let fee = expected_fee(tranche, fee_bps);
        let net = tranche - fee;
        assert_gross_conserved(tranche, fee, net);
        cumulative_fee += fee;
        cumulative_net += net;
        cumulative_gross += tranche;
    }

    assert_eq!(cumulative_gross, amount);
    assert_eq!(cumulative_fee + cumulative_net, amount);
    // Per-tranche floor rounding is the approved #224 outcome.
    assert_eq!(cumulative_fee, expected_fee(333, fee_bps) * 2 + expected_fee(334, fee_bps));

    client.release(&sme);
    assert_eq!(client.get_escrow().fee_amount, expected_fee(amount, fee_bps));
    assert_eq!(client.get_escrow().net_amount, amount - expected_fee(amount, fee_bps));
}

#[test]
fn cumulative_partial_withdrawals_match_approved_rounding() {
    let env = Env::default();
    env.mock_all_auths();
    let amount = 1_000i128;
    let fee_bps = 250i64;
    let (client, _admin, sme, _treasury) = init_fee_split_test(&env, amount, fee_bps);

    let tranches = [333i128, 333i128, 334i128];
    let mut cumulative_fee = 0i128;
    let mut cumulative_net = 0i128;
    let mut cumulative_gross = 0i128;

    for tranche in tranches {
        let fee = expected_fee(tranche, fee_bps);
        let net = tranche - fee;
        assert_gross_conserved(tranche, fee, net);
        cumulative_fee += fee;
        cumulative_net += net;
        cumulative_gross += tranche;
    }

    assert_eq!(cumulative_gross, amount);
    assert_eq!(cumulative_fee + cumulative_net, amount);
    assert_eq!(cumulative_fee, expected_fee(333, fee_bps) * 2 + expected_fee(334, fee_bps));

    client.withdraw(&sme);
    assert_eq!(client.get_escrow().fee_amount, expected_fee(amount, fee_bps));
    assert_eq!(client.get_escrow().net_amount, amount - expected_fee(amount, fee_bps));
}
