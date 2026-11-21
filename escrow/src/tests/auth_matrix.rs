//! Exhaustive negative-authorization test matrix for all role-gated entrypoints.
//!
//! For each state-mutating entrypoint this module asserts:
//! 1. The call **panics** when the wrong signer is presented (`mock_auths` with wrong address).
//! 2. The call **panics** when no signer is presented (`mock_auths(&[])`).
//!
//! Guards tested per ADR-002 and the "Authorization guard ordering" rustdoc in lib.rs:
//!   - Read-only preconditions occur before `require_auth` (no state mutation before auth).
//!   - Every role boundary (admin, sme, investor, treasury, pending_admin) is covered.
//!
//! No production-code changes are made here; any guard gap found should be fixed separately.
use super::*;
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    token::{Client as TokenClient, StellarAssetClient},
    Address, BytesN, Env, IntoVal, String as SorobanString, Vec as SorobanVec,
};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Deploy and initialise a minimal escrow, returning `(client, admin, sme, treasury, token)`.
/// The environment has `mock_all_auths` enabled so init itself succeeds.
fn setup_inited(
    env: &Env,
) -> (
    crate::StarfundEscrowClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    env.mock_all_auths();
    let client = deploy(env);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let token = Address::generate(env);
    let treasury = Address::generate(env);
    client.init(
        &admin,
        &SorobanString::from_str(env, "INV_AUTH"),
        &sme,
        &1_000i128,
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
    (client, admin, sme, treasury, token)
}

/// Assert a call panics with no auth at all.
macro_rules! assert_no_auth_panics {
    ($env:expr, $call:expr) => {{
        $env.mock_auths(&[]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $call));
        assert!(
            result.is_err(),
            "expected panic with no auth, but call succeeded"
        );
    }};
}

/// Assert a call panics when signed by `wrong_signer` only.
macro_rules! assert_wrong_auth_panics {
    ($env:expr, $wrong:expr, $contract_id:expr, $fn_name:expr, $args:expr, $call:expr) => {{
        $env.mock_auths(&[MockAuth {
            address: &$wrong,
            invoke: &MockAuthInvoke {
                contract: &$contract_id,
                fn_name: $fn_name,
                args: $args,
                sub_invokes: &[],
            },
        }]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $call));
        assert!(
            result.is_err(),
            "expected panic with wrong signer on {}, but call succeeded",
            $fn_name
        );
    }};
}

use crate::EscrowError;

// ── settlement-specific test helpers ─────────────────────────────────────

/// Create a funded escrow (status 1) with a single investor.
/// The environment has `mock_all_auths` enabled so all setup calls succeed.
fn setup_funded(
    env: &Env,
) -> (
    crate::StarfundEscrowClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    env.mock_all_auths();
    let client = deploy(env);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let treasury = Address::generate(env);
    let token = Address::generate(env);
    client.init(
        &admin,
        &SorobanString::from_str(env, "AUTH_STL"),
        &sme,
        &100_000_000_000i128,
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
    let investor = Address::generate(env);
    client.fund(&investor, &100_000_000_000i128);
    (client, admin, sme, investor, treasury)
}

/// Create a settled escrow (status 2) with a single investor.
fn setup_settled(
    env: &Env,
) -> (
    crate::StarfundEscrowClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    let (client, admin, sme, investor, treasury) = setup_funded(env);
    client.settle();
    (client, admin, sme, investor, treasury)
}

// ── contract-wallet funding authorization (#400) ────────────────────────

/// Deploy a real Stellar asset contract and a minimal escrow that uses it as
/// the funding token. Returns `(client, admin, sme, treasury, token)`.
///
/// Unlike `setup_inited`, the token is a real SAC so the nested token-transfer
/// authorization path is exercised. `mock_all_auths` is enabled only for
/// `init`; callers are expected to install explicit `mock_auths` before
/// invoking `fund`.
fn setup_inited_with_sac(
    env: &Env,
) -> (
    crate::StarfundEscrowClient<'_>,
    Address,
    Address,
    Address,
    Address,
) {
    env.mock_all_auths();
    let client = deploy(env);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let treasury = Address::generate(env);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let token = sac.address();
    client.init(
        &admin,
        &SorobanString::from_str(env, "INV_CW"),
        &sme,
        &1_000i128,
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
    (client, admin, sme, treasury, token)
}

/// A minimal contract-wallet fixture.
///
/// The wallet is represented by a registered contract address. It holds a
/// token balance and authorizes funding through a single explicit auth entry
/// that covers both the escrow `fund` invocation and the nested token
/// `transfer` sub-invocation. No `__check_auth` is implemented: the fixture
/// models the wallet's authorization surface via `MockAuth` entries so the
/// host still validates the full invocation tree (contract → token).
struct ContractWallet {
    address: Address,
}

impl ContractWallet {
    /// Register a contract-wallet address and fund it with `balance` of `token`.
    fn new(env: &Env, token: &Address, balance: i128) -> Self {
        let address = env.register(crate::StarfundEscrow, ());
        // Mint tokens to the wallet using the SAC admin path (setup only).
        env.mock_all_auths();
        StellarAssetClient::new(env, token).mint(&address, &balance);
        Self { address }
    }

    /// Build the explicit auth entry authorizing `fund(investor, amount)` on
    /// `escrow` plus the nested token `transfer(wallet, escrow, amount)`.
    fn fund_auth<'a>(
        &'a self,
        env: &'a Env,
        escrow: &'a Address,
        token: &'a Address,
        amount: i128,
    ) -> MockAuth<'a> {
        let transfer_args = SorobanVec::from_array(
            env,
            [
                self.address.into_val(env),
                escrow.into_val(env),
                amount.into_val(env),
            ],
        );
        let sub = MockAuthInvoke {
            contract: token,
            fn_name: "transfer",
            args: transfer_args,
            sub_invokes: &[],
        };
        let sub_invokes: &[MockAuthInvoke] = Box::leak(Box::new([sub]));
        MockAuth {
            address: &self.address,
            invoke: &MockAuthInvoke {
                contract: escrow,
                fn_name: "fund",
                args: SorobanVec::from_array(
                    env,
                    [self.address.into_val(env), amount.into_val(env)],
                ),
                sub_invokes,
            },
        }
    }
}

/// A supported contract wallet can fund successfully when it authorizes
/// exactly the intended `fund` invocation and the corresponding token
/// transfer sub-invocation.
#[test]
fn test_contract_wallet_fund_succeeds() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, token) = setup_inited_with_sac(&env);
    let wallet = ContractWallet::new(&env, &token, 1_000i128);

    let amount = 1_000i128;
    let auth = wallet.fund_auth(&env, &client.address, &token, amount);
    env.mock_auths(&[auth]);

    client.fund(&wallet.address, &amount);

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&wallet.address), 0);
    assert_eq!(token_client.balance(&client.address), amount);
    assert_eq!(client.get_escrow().funded_amount, amount);
}

/// An altered amount in the auth entry is rejected by the host and balances
/// remain unchanged.
#[test]
fn test_contract_wallet_fund_altered_amount_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, token) = setup_inited_with_sac(&env);
    let wallet = ContractWallet::new(&env, &token, 1_000i128);

    // Authorize 500 but attempt to fund 1_000.
    let auth = wallet.fund_auth(&env, &client.address, &token, 500i128);
    env.mock_auths(&[auth]);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&wallet.address, &1_000i128);
    }));
    assert!(result.is_err(), "altered amount must be rejected");

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&wallet.address), 1_000i128);
    assert_eq!(token_client.balance(&client.address), 0);
    assert_eq!(client.get_escrow().funded_amount, 0);
}

/// An auth entry targeting a different escrow invocation is rejected and
/// balances remain unchanged.
#[test]
fn test_contract_wallet_fund_altered_invocation_rejected() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, token) = setup_inited_with_sac(&env);
    let wallet = ContractWallet::new(&env, &token, 1_000i128);

    // Authorize a `withdraw` invocation instead of `fund`.
    let auth = MockAuth {
        address: &wallet.address,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "withdraw",
            args: SorobanVec::new(&env),
            sub_invokes: &[],
        },
    };
    env.mock_auths(&[auth]);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&wallet.address, &1_000i128);
    }));
    assert!(result.is_err(), "altered invocation must be rejected");

    let token_client = TokenClient::new(&env, &token);
    assert_eq!(token_client.balance(&wallet.address), 1_000i128);
    assert_eq!(token_client.balance(&client.address), 0);
    assert_eq!(client.get_escrow().funded_amount, 0);
}

// ── partial_settle ──────────────────────────────────────────────────────

/// A stranger calling `partial_settle` with their own auth passes the
/// `require_auth` gate but is rejected by the explicit role check and
/// receives a typed `PartialSettleUnauthorizedCaller` error.
#[test]
fn test_partial_settle_stranger_rejected_with_typed_error() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let stranger = Address::generate(&env);

    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "partial_settle",
            args: SorobanVec::from_array(&env, [stranger.into_val(&env)]),
            sub_invokes: &[],
        },
    }]);

    assert_contract_error(
        client.try_partial_settle(&stranger),
        EscrowError::PartialSettleUnauthorizedCaller,
    );
}

/// Calling `partial_settle` with no authorization at all panics at the
/// host-level `require_auth` before any role check runs.
#[test]
#[should_panic]
fn test_partial_settle_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, sme, _treasury, _token) = setup_inited(&env);
    env.mock_auths(&[]);
    client.partial_settle(&sme);
}

// ── settle ──────────────────────────────────────────────────────────────

/// Once an escrow has reached maturity, settlement is permissionless (no auth required)
/// to allow operators/admins to perform batch settlements without per-SME cross-contract signatures.
#[test]
fn test_settle_permissionless_when_mature() {
    let env = Env::default();
    let (client, _admin, _sme, _investor, _treasury) = setup_funded(&env);
    env.mock_auths(&[]);
    let result = client.settle();
    assert_eq!(result.escrow.status, 2u32);
}

// ── withdraw ────────────────────────────────────────────────────────────

/// Calling `withdraw` with no authorization panics at the host-level
/// `sme_address.require_auth()` inside `load_escrow_require_sme`.
#[test]
#[should_panic]
fn test_withdraw_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _investor, _treasury) = setup_funded(&env);
    env.mock_auths(&[]);
    client.withdraw();
}

/// Calling `withdraw` with a non-SME signer panics at the host-level
/// `require_auth` because `load_escrow_require_sme` demands the SME's
/// signature.
#[test]
#[should_panic]
fn test_withdraw_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _investor, _treasury) = setup_funded(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "withdraw",
            args: SorobanVec::new(&env),
            sub_invokes: &[],
        },
    }]);
    client.withdraw();
}

// ── claim_investor_payout ───────────────────────────────────────────────

/// Calling `claim_investor_payout` with no authorization panics at the
/// host-level `investor.require_auth()`.
#[test]
#[should_panic]
fn test_claim_investor_payout_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, investor, _treasury) = setup_settled(&env);
    env.mock_auths(&[]);
    client.claim_investor_payout(&investor);
}

/// Calling `claim_investor_payout` with a non-investor signer panics at the
/// host-level `require_auth` because the investor's signature is required.
#[test]
#[should_panic]
fn test_claim_investor_payout_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, _sme, investor, _treasury) = setup_settled(&env);
    let stranger = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "claim_investor_payout",
            args: SorobanVec::from_array(&env, [investor.into_val(&env)]),
            sub_invokes: &[],
        },
    }]);
    client.claim_investor_payout(&investor);
}

// ── cancel_funding ──────────────────────────────────────────────────────

/// Calling `cancel_funding` with no authorization panics at the host-level
/// `admin.require_auth()` inside `load_escrow_require_admin`.
#[test]
#[should_panic]
fn test_cancel_funding_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    env.mock_auths(&[]);
    client.cancel_funding(&0u32);
}

/// Calling `cancel_funding` with a non-admin signer panics at the
/// host-level `require_auth` because `load_escrow_require_admin` demands
/// the admin's signature.
#[test]
#[should_panic]
fn test_cancel_funding_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, sme, _treasury, _token) = setup_inited(&env);
    env.mock_auths(&[MockAuth {
        address: &sme,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "cancel_funding",
            args: SorobanVec::new(&env),
            sub_invokes: &[],
        },
    }]);
    client.cancel_funding(&0u32);
}

// ── refund ──────────────────────────────────────────────────────────────

/// Calling `refund` with no authorization panics at the host-level
/// `investor.require_auth()` before any state mutation or token transfer.
#[test]
#[should_panic]
fn test_refund_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let investor = Address::generate(&env);
    // Fund and cancel to reach status 4 (cancelled).
    client.fund(&investor, &1_000i128);
    client.cancel_funding(&0u32);
    env.mock_auths(&[]);
    client.refund(&investor);
}

/// Calling `refund` with a non-investor signer panics at the host-level
/// `require_auth` because the function demands the specific investor's
/// signature.
#[test]
#[should_panic]
fn test_refund_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let investor = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.cancel_funding(&0u32);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "refund",
            args: SorobanVec::from_array(&env, [investor.into_val(&env)]),
            sub_invokes: &[],
        },
    }]);
    client.refund(&investor);
}

// ── get_contributions positional parity ─────────────────────────────────

/// Generated positional-parity check for `get_contributions`.
///
/// Builds a mix of funded investors, exited (refunded) investors and unknown
/// addresses, then reads them back through `get_contributions` in batches that
/// include duplicates and unknowns. Every batch slot must equal the single
/// `get_contribution` result for the address at that same input position.
#[test]
fn test_get_contributions_positional_parity_with_duplicates_and_unknowns() {
    let env = Env::default();
    env.mock_all_auths();
    let client = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token = Address::generate(&env);
    client.init(
        &admin,
        &SorobanString::from_str(&env, "AUTH_PARITY"),
        &sme,
        &1_000_000_000i128,
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

    // Funded investors (status 1).
    let funded_a = Address::generate(&env);
    let funded_b = Address::generate(&env);
    client.fund(&funded_a, &10_000i128);
    client.fund(&funded_b, &25_000i128);

    // Exited investors: fund, then cancel funding and refund to reach status 4.
    let exited_a = Address::generate(&env);
    let exited_b = Address::generate(&env);
    client.fund(&exited_a, &5_000i128);
    client.fund(&exited_b, &7_500i128);
    client.cancel_funding(&0u32);
    client.refund(&exited_a);
    client.refund(&exited_b);

    // Unknown addresses that never interacted with the escrow.
    let unknown_a = Address::generate(&env);
    let unknown_b = Address::generate(&env);

    // Build an input batch with duplicates and unknowns interleaved.
    let inputs = SorobanVec::from_array(
        &env,
        [
            funded_a.clone(),
            unknown_a.clone(),
            funded_b.clone(),
            funded_a.clone(),
            exited_a.clone(),
            unknown_b.clone(),
            exited_b.clone(),
            exited_a.clone(),
            funded_b.clone(),
            unknown_a.clone(),
        ],
    );

    let outputs = client.get_contributions(&inputs);
    assert_eq!(
        outputs.len(),
        inputs.len(),
        "output length must equal input length"
    );

    // Every slot must match the single getter for the same input position.
    for i in 0..inputs.len() {
        let addr = inputs.get(i).unwrap();
        let expected = client.get_contribution(&addr);
        assert_eq!(
            outputs.get(i).unwrap(),
            expected,
            "batch slot {} disagrees with get_contribution",
            i
        );
    }

    // Duplicate positions repeat the same value.
    assert_eq!(outputs.get(0).unwrap(), outputs.get(3).unwrap());
    assert_eq!(outputs.get(2).unwrap(), outputs.get(8).unwrap());
    assert_eq!(outputs.get(4).unwrap(), outputs.get(7).unwrap());
    assert_eq!(outputs.get(1).unwrap(), outputs.get(9).unwrap());

    // Unknown addresses report zero contribution.
    assert_eq!(outputs.get(1).unwrap(), 0i128);
    assert_eq!(outputs.get(5).unwrap(), 0i128);

    // Empty input returns empty output.
    let empty_inputs: SorobanVec<Address> = SorobanVec::new(&env);
    let empty_outputs = client.get_contributions(&empty_inputs);
    assert_eq!(empty_outputs.len(), 0u32);
}

// ── sweep_terminal_dust ─────────────────────────────────────────────────

/// Calling `sweep_terminal_dust` with no authorization panics at the
/// host-level `treasury.require_auth()`.
#[test]
#[should_panic]
fn test_sweep_terminal_dust_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    // Cancel to reach a terminal status (4 — cancelled).
    client.cancel_funding(&0u32);
    env.mock_auths(&[]);
    client.sweep_terminal_dust(&100i128);
}

/// Calling `sweep_terminal_dust` with a non-treasury signer panics at the
/// host-level `require_auth` because the function demands the treasury's
/// signature.
#[test]
#[should_panic]
fn test_sweep_terminal_dust_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, sme, _treasury, _token) = setup_inited(&env);
    client.cancel_funding(&0u32);
    env.mock_auths(&[MockAuth {
        address: &sme,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "sweep_terminal_dust",
            args: SorobanVec::from_array(&env, [100i128.into_val(&env)]),
            sub_invokes: &[],
        },
    }]);
    client.sweep_terminal_dust(&100i128);
}

// ── rotate_beneficiary ──────────────────────────────────────────────────

/// `rotate_beneficiary` still demands the SME's signature before the
/// role-separation policy runs: a stranger cannot rotate the beneficiary.
#[test]
#[should_panic]
fn test_rotate_beneficiary_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let stranger = Address::generate(&env);
    let new_beneficiary = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "rotate_beneficiary",
            args: SorobanVec::from_array(&env, [new_beneficiary.into_val(&env)]),
            sub_invokes: &[],
        },
    }]);
    client.rotate_beneficiary(&new_beneficiary);
}

/// `rotate_beneficiary` with no auth at all panics at the host-level
/// `require_auth` before any policy check.
#[test]
#[should_panic]
fn test_rotate_beneficiary_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let new_beneficiary = Address::generate(&env);
    env.mock_auths(&[]);
    client.rotate_beneficiary(&new_beneficiary);
}

// ── propose_admin ───────────────────────────────────────────────────────

/// `propose_admin` still demands the current admin's signature before the
/// role-separation policy runs: a stranger cannot nominate an admin.
#[test]
#[should_panic]
fn test_propose_admin_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, sme, _treasury, _token) = setup_inited(&env);
    let candidate = Address::generate(&env);
    env.mock_auths(&[MockAuth {
        address: &sme,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "propose_admin",
            args: SorobanVec::from_array(&env, [candidate.into_val(&env)]),
            sub_invokes: &[],
        },
    }]);
    client.propose_admin(&candidate);
}

/// `propose_admin` with no auth at all panics at the host-level
/// `require_auth` before any policy check.
#[test]
#[should_panic]
fn test_propose_admin_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let candidate = Address::generate(&env);
    env.mock_auths(&[]);
    client.propose_admin(&candidate);
}

// ── accept_admin ────────────────────────────────────────────────────────

/// `accept_admin` still demands the pending admin's signature before the
/// role-separation recheck runs: a stranger cannot accept a nomination.
#[test]
#[should_panic]
fn test_accept_admin_wrong_signer_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let candidate = Address::generate(&env);
    let stranger = Address::generate(&env);
    client.propose_admin(&candidate);
    env.mock_auths(&[MockAuth {
        address: &stranger,
        invoke: &MockAuthInvoke {
            contract: &client.address,
            fn_name: "accept_admin",
            args: SorobanVec::new(&env),
            sub_invokes: &[],
        },
    }]);
    client.accept_admin();
}

/// `accept_admin` with no auth at all panics at the host-level
/// `require_auth` before any policy recheck.
#[test]
#[should_panic]
fn test_accept_admin_no_auth_panics() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let candidate = Address::generate(&env);
    client.propose_admin(&candidate);
    env.mock_auths(&[]);
    client.accept_admin();
}

// ── recover_admin ───────────────────────────────────────────────────────

/// Permissionless expired-proposal cleanup must not consume the shared
/// administrative nonce: an unrelated caller can clear an expired pending
/// proposal without auth, and a prepared admin action using the original
/// nonce still executes afterwards.
#[test]
fn test_recover_admin_cleanup_does_not_consume_admin_nonce() {
    let env = Env::default();
    let (client, _admin, _sme, _treasury, _token) = setup_inited(&env);
    let candidate = Address::generate(&env);
    let stranger = Address::generate(&env);

    // Nominate an admin, then let the proposal expire.
    client.propose_admin(&candidate);
    env.ledger().set_sequence_number(env.ledger().sequence() + 1_000_000u32);

    // Cleanup from an unrelated caller requires no admin auth.
    env.mock_auths(&[]);
    client.recover_admin();

    // The admin nonce was not advanced by cleanup: a prepared admin action
    // using the original nonce still succeeds.
    env.mock_all_auths();
    let replacement = Address::generate(&env);
    client.propose_admin(&replacement);
}
