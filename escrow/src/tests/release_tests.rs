use crate::{
    tests::{assert_contract_error, init_and_fund_with_real_token, TARGET},
    EscrowError, FinalRelease, PartialRelease, StarfundEscrow, StarfundEscrowClient,
};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Events},
    vec, Address, Env, IntoVal, String,
};

#[test]
fn test_release_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _) = init_and_fund_with_real_token(&env, TARGET, "INV001");
    assert_contract_error(
        client.try_release(&0),
        EscrowError::ReleaseAmountNotPositive,
    );
}

#[test]
fn test_release_exact_remaining() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, escrow_id, sme) = init_and_fund_with_real_token(&env, TARGET, "INV001");

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_sme_balance = token_client.balance(&sme);
    let init_escrow_balance = token_client.balance(&escrow_id);

    client.release(&TARGET);

    let final_sme_balance = token_client.balance(&sme);
    assert_eq!(final_sme_balance, init_sme_balance + TARGET);
    assert_eq!(
        token_client.balance(&escrow_id),
        init_escrow_balance - TARGET
    );

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn
}

#[test]
fn test_release_above_remaining() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _) = init_and_fund_with_real_token(&env, TARGET, "INV001");

    assert_contract_error(
        client.try_release(&(TARGET + 1)),
        EscrowError::ReleaseExceedsRemaining,
    );
}

#[test]
fn test_two_partial_releases_race() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, escrow_id, sme) = init_and_fund_with_real_token(&env, TARGET, "INV001");

    let p1 = TARGET / 2;
    client.release(&p1);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 1); // Still funded

    let p2 = TARGET - p1;
    client.release(&p2);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn
}

#[test]
fn test_final_release_repeated() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _, _) = init_and_fund_with_real_token(&env, TARGET, "INV001");

    client.release(&TARGET);

    // Escrow is now status 3, so release should fail with ReleaseNotFunded
    assert_contract_error(client.try_release(&TARGET), EscrowError::ReleaseNotFunded);
}

#[test]
fn test_release_unauthorized() {
    let env = Env::default();
    let (client, _, _) = init_and_fund_with_real_token(&env, TARGET, "INV001");

    // without mock_all_auths, this should fail with auth error.
    let res = client.try_release(&TARGET);
    assert!(res.is_err());
}

/// Helper to init and fund an escrow with protocol fees
fn init_and_fund_with_protocol_fee<'a>(
    env: &'a Env,
    target: i128,
    invoice_id: &str,
    protocol_fee_bps: i64,
) -> (StarfundEscrowClient<'a>, Address, Address, Address, Address) {
    let sac = env.register_stellar_asset_contract_v2(Address::generate(env));
    let token_id = sac.address();
    let sac_admin = StellarAssetClient::new(env, &token_id);

    let escrow_id = env.register(StarfundEscrow, ());
    let client = StarfundEscrowClient::new(env, &escrow_id);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let treasury = Address::generate(env);

    client.init(
        &admin,
        &String::from_str(env, invoice_id),
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
        &Some(protocol_fee_bps),
        &None::<u32>,
    );

    let investor = Address::generate(env);
    sac_admin.mint(&investor, &target);
    client.fund(&investor, &target);

    sac_admin.mint(&escrow_id, &target);

    (client, escrow_id, sme, treasury, investor)
}

#[test]
fn test_release_with_protocol_fee_zero_fee_escrow() {
    let env = Env::default();
    env.mock_all_auths();
    // When protocol_fee_bps is 0, release() should transfer 100% to SME (like before)
    let (client, escrow_id, sme, treasury, _investor) =
        init_and_fund_with_protocol_fee(&env, TARGET, "INV002", 0);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_sme_balance = token_client.balance(&sme);
    let init_treasury_balance = token_client.balance(&treasury);
    let init_escrow_balance = token_client.balance(&escrow_id);

    client.release(&TARGET);

    let final_sme_balance = token_client.balance(&sme);
    let final_treasury_balance = token_client.balance(&treasury);
    let final_escrow_balance = token_client.balance(&escrow_id);

    // With 0% fee, SME gets 100% of the amount
    assert_eq!(final_sme_balance, init_sme_balance + TARGET);
    // Treasury receives no fee
    assert_eq!(final_treasury_balance, init_treasury_balance);
    assert_eq!(final_escrow_balance, init_escrow_balance - TARGET);
}

#[test]
fn test_release_with_protocol_fee_positive_fee() {
    let env = Env::default();
    env.mock_all_auths();
    // With 250 bps (2.5%), release() should split the amount
    let (client, escrow_id, sme, treasury, _investor) =
        init_and_fund_with_protocol_fee(&env, TARGET, "INV003", 250);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_sme_balance = token_client.balance(&sme);
    let init_treasury_balance = token_client.balance(&treasury);
    let init_escrow_balance = token_client.balance(&escrow_id);

    client.release(&TARGET);

    let final_sme_balance = token_client.balance(&sme);
    let final_treasury_balance = token_client.balance(&treasury);
    let final_escrow_balance = token_client.balance(&escrow_id);

    // Compute expected fee and net using the shared checked fee split policy
    let fee = TARGET * 250 / 10_000;
    let net = TARGET - fee;
    // Gross must equal fee plus net
    assert_eq!(fee + net, TARGET);

    // SME receives the net amount
    assert_eq!(final_sme_balance, init_sme_balance + net);
    // Treasury receives the fee
    assert_eq!(final_treasury_balance, init_treasury_balance + fee);
    // Escrow balance decreases by the full gross amount
    assert_eq!(final_escrow_balance, init_escrow_balance - TARGET);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn
}

#[test]
fn test_release_with_protocol_fee_partial_then_final() {
    let env = Env::default();
    env.mock_all_auths();
    // With 500 bps (5%), test partial + final releases
    let (client, escrow_id, sme, treasury, _investor) =
        init_and_fund_with_protocol_fee(&env, TARGET, "INV004", 500);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_sme_balance = token_client.balance(&sme);
    let init_treasury_balance = token_client.balance(&treasury);

    // First release: 40% of TARGET
    let p1 = TARGET / 2;
    client.release(&p1);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 1); // Still funded

    // Compute expected fee and net for first release
    let fee1 = p1 * 500 / 10_000;
    let net1 = p1 - fee1;
    assert_eq!(fee1 + net1, p1);

    let sme_balance_after_first = token_client.balance(&sme);
    let treasury_balance_after_first = token_client.balance(&treasury);

    assert_eq!(sme_balance_after_first, init_sme_balance + net1);
    assert_eq!(treasury_balance_after_first, init_treasury_balance + fee1);

    // Second release: remaining 60% of TARGET
    let p2 = TARGET - p1;
    client.release(&p2);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn

    // Compute expected fee and net for second release
    let fee2 = p2 * 500 / 10_000;
    let net2 = p2 - fee2;
    assert_eq!(fee2 + net2, p2);

    let final_sme_balance = token_client.balance(&sme);
    let final_treasury_balance = token_client.balance(&treasury);

    // Total SME balance should be init + net1 + net2
    assert_eq!(final_sme_balance, init_sme_balance + net1 + net2);
    // Total treasury balance should be init + fee1 + fee2
    assert_eq!(final_treasury_balance, init_treasury_balance + fee1 + fee2);

    // Conservation check: net1 + net2 + fee1 + fee2 == p1 + p2 == TARGET
    assert_eq!(net1 + net2 + fee1 + fee2, TARGET);
}

#[test]
fn test_release_with_protocol_fee_max_fee() {
    let env = Env::default();
    env.mock_all_auths();
    // With 10_000 bps (100%), Treasury gets everything, SME gets 0
    let (client, escrow_id, sme, treasury, _investor) =
        init_and_fund_with_protocol_fee(&env, TARGET, "INV005", 10_000);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_sme_balance = token_client.balance(&sme);
    let init_treasury_balance = token_client.balance(&treasury);
    let init_escrow_balance = token_client.balance(&escrow_id);

    client.release(&TARGET);

    let final_sme_balance = token_client.balance(&sme);
    let final_treasury_balance = token_client.balance(&treasury);
    let final_escrow_balance = token_client.balance(&escrow_id);

    // At 100% fee: fee == TARGET, net == 0
    let fee = TARGET * 10_000 / 10_000;
    let net = TARGET - fee;
    assert_eq!(fee + net, TARGET);
    assert_eq!(fee, TARGET);
    assert_eq!(net, 0);

    // SME receives nothing
    assert_eq!(final_sme_balance, init_sme_balance);
    // Treasury receives everything
    assert_eq!(final_treasury_balance, init_treasury_balance + TARGET);
    assert_eq!(final_escrow_balance, init_escrow_balance - TARGET);
}

/// Helper to init and fund an escrow where the treasury address is aliased to
/// the SME address, with a nonzero protocol fee.
fn init_and_fund_with_aliased_recipient<'a>(
    env: &'a Env,
    target: i128,
    invoice_id: &str,
    protocol_fee_bps: i64,
) -> (StarfundEscrowClient<'a>, Address, Address) {
    let sac = env.register_stellar_asset_contract_v2(Address::generate(env));
    let token_id = sac.address();
    let sac_admin = StellarAssetClient::new(env, &token_id);

    let escrow_id = env.register(StarfundEscrow, ());
    let client = StarfundEscrowClient::new(env, &escrow_id);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    // Treasury aliases the SME address.
    let treasury = sme.clone();

    client.init(
        &admin,
        &String::from_str(env, invoice_id),
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
        &Some(protocol_fee_bps),
        &None::<u32>,
    );

    let investor = Address::generate(env);
    sac_admin.mint(&investor, &target);
    client.fund(&investor, &target);

    sac_admin.mint(&escrow_id, &target);

    (client, escrow_id, sme)
}

#[test]
fn test_release_with_aliased_treasury_full() {
    let env = Env::default();
    env.mock_all_auths();
    // Treasury == SME, nonzero fee. The aliased recipient must receive the
    // gross amount exactly once (net + fee both land on the same address).
    let (client, escrow_id, recipient) =
        init_and_fund_with_aliased_recipient(&env, TARGET, "INV007", 250);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_recipient_balance = token_client.balance(&recipient);
    let init_escrow_balance = token_client.balance(&escrow_id);

    client.release(&TARGET);

    let final_recipient_balance = token_client.balance(&recipient);
    let final_escrow_balance = token_client.balance(&escrow_id);

    // Aliased recipient receives gross exactly once.
    assert_eq!(final_recipient_balance, init_recipient_balance + TARGET);
    // Escrow decrease equals gross.
    assert_eq!(final_escrow_balance, init_escrow_balance - TARGET);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn
}

#[test]
fn test_release_with_aliased_treasury_partial_then_final() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, escrow_id, recipient) =
        init_and_fund_with_aliased_recipient(&env, TARGET, "INV008", 500);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_recipient_balance = token_client.balance(&recipient);
    let init_escrow_balance = token_client.balance(&escrow_id);

    let p1 = TARGET / 2;
    client.release(&p1);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 1); // Still funded

    let recipient_after_first = token_client.balance(&recipient);
    let escrow_after_first = token_client.balance(&escrow_id);

    // Aliased recipient receives the full gross of the first release.
    assert_eq!(recipient_after_first, init_recipient_balance + p1);
    assert_eq!(escrow_after_first, init_escrow_balance - p1);

    let p2 = TARGET - p1;
    client.release(&p2);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn

    let final_recipient_balance = token_client.balance(&recipient);
    let final_escrow_balance = token_client.balance(&escrow_id);

    // Total recipient increase equals gross release across both transfers.
    assert_eq!(final_recipient_balance, init_recipient_balance + TARGET);
    assert_eq!(final_escrow_balance, init_escrow_balance - TARGET);
}

#[test]
fn test_withdraw_with_aliased_treasury() {
    let env = Env::default();
    env.mock_all_auths();
    // Treasury == SME, nonzero fee, exercise the withdraw path.
    let (client, escrow_id, recipient) =
        init_and_fund_with_aliased_recipient(&env, TARGET, "INV009", 250);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_recipient_balance = token_client.balance(&recipient);
    let init_escrow_balance = token_client.balance(&escrow_id);

    client.withdraw();

    let final_recipient_balance = token_client.balance(&recipient);
    let final_escrow_balance = token_client.balance(&escrow_id);

    // Aliased recipient receives gross exactly once.
    assert_eq!(final_recipient_balance, init_recipient_balance + TARGET);
    // Escrow decrease equals gross.
    assert_eq!(final_escrow_balance, init_escrow_balance - TARGET);

    let escrow = client.get_escrow();
    assert_eq!(escrow.status, 3); // Withdrawn
}

#[test]
fn test_release_with_protocol_fee_rounding_residue() {
    let env = Env::default();
    env.mock_all_auths();
    // Test that rounding residue stays with SME
    // With 333 bps (3.33%), on certain amounts there may be floor residue
    let (client, escrow_id, sme, treasury, _investor) =
        init_and_fund_with_protocol_fee(&env, TARGET, "INV006", 333);

    let token = client.funding_token();
    let token_client = TokenClient::new(&env, &token);

    let init_sme_balance = token_client.balance(&sme);
    let init_treasury_balance = token_client.balance(&treasury);

    client.release(&TARGET);

    let final_sme_balance = token_client.balance(&sme);
    let final_treasury_balance = token_client.balance(&treasury);

    // Compute expected fee (with floor)
    let fee = TARGET * 333 / 10_000; // floor division
    let net = TARGET - fee;

    assert_eq!(final_sme_balance, init_sme_balance + net);
    assert_eq!(final_treasury_balance, init_treasury_balance + fee);

    // Verify conservation: net + fee == TARGET
    assert_eq!(net + fee, TARGET);
}

#[test]
fn test_release_and_withdraw_fee_split_identical() {
    let env = Env::default();
    env.mock_all_auths();

    // Same gross amount and fee rate through both disbursement routes.
    let gross = TARGET;
    let fee_bps = 333i64;

    // Route 1: release
    let (release_client, _, release_sme, release_treasury, _) =
        init_and_fund_with_protocol_fee(&env, gross, "INV007", fee_bps);
    let release_token = release_client.funding_token();
    let release_token_client = TokenClient::new(&env, &release_token);

    let release_sme_before = release_token_client.balance(&release_sme);
    let release_treasury_before = release_token_client.balance(&release_treasury);

    release_client.release(&gross);

    let release_fee = release_token_client.balance(&release_treasury) - release_treasury_before;
    let release_net = release_token_client.balance(&release_sme) - release_sme_before;

    // Route 2: withdraw
    let (withdraw_client, _, withdraw_sme, withdraw_treasury, _) =
        init_and_fund_with_protocol_fee(&env, gross, "INV008", fee_bps);
    let withdraw_token = withdraw_client.funding_token();
    let withdraw_token_client = TokenClient::new(&env, &withdraw_token);

    let withdraw_sme_before = withdraw_token_client.balance(&withdraw_sme);
    let withdraw_treasury_before = withdraw_token_client.balance(&withdraw_treasury);

    withdraw_client.withdraw();

    let withdraw_fee = withdraw_token_client.balance(&withdraw_treasury) - withdraw_treasury_before;
    let withdraw_net = withdraw_token_client.balance(&withdraw_sme) - withdraw_sme_before;

    // Both routes must share the same checked fee arithmetic.
    assert_eq!(release_fee, withdraw_fee);
    assert_eq!(release_net, withdraw_net);

    // Gross equals fee plus net on both routes.
    assert_eq!(release_fee + release_net, gross);
    assert_eq!(withdraw_fee + withdraw_net, gross);

    // Existing floor rounding policy is preserved.
    let expected_fee = gross * fee_bps / 10_000;
    assert_eq!(release_fee, expected_fee);
    assert_eq!(withdraw_fee, expected_fee);
}
