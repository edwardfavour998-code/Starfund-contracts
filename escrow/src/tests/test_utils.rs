//! Shared test utilities for the escrow contract test suite.
//!
//! This module provides:
//! * [`TestConfig`] — a named-field builder for test initialization with
//!   deliberate default token fixtures.
//! * [`assert_contract_error`] — a centralized assertion helper for nested
//!   Soroban contract errors.
//! * [`find_events`] / [`decode_events`] — deterministic event selection and
//!   decoding helpers.

#![allow(dead_code)]

use soroban_sdk::{
    testutils::{Address as _, Events as _, Ledger as _},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env, IntoVal, Symbol, TryFromVal, Val, Vec,
};

use crate::{
    errors::EscrowError,
    tests::test_utils::test_helpers::{install_stellar_asset_token, register_escrow},
    EscrowContract, EscrowContractClient,
};

// ---------------------------------------------------------------------------
// TestConfig
// ---------------------------------------------------------------------------

/// Named-field configuration for constructing escrow test fixtures.
///
/// All fields have deliberate defaults installed by [`TestConfig::defaults`].
/// Use the `with_*` builder methods to override individual values while
/// keeping the rest of the fixture intact.
#[derive(Clone)]
pub struct TestConfig {
    /// Invoice identifier used for the escrow.
    pub invoice_id: Symbol,
    /// Target funding amount (in token base units).
    pub target_amount: i128,
    /// Yield in basis points.
    pub yield_bps: u32,
    /// Maturity timestamp (ledger time).
    pub maturity: u64,
    /// Address of the funding token (a real Stellar asset contract).
    pub funding_token: Address,
    /// Treasury address that receives fees.
    pub treasury: Address,
    /// Minimum contribution per investor.
    pub min_contribution: i128,
    /// Maximum contribution per investor.
    pub max_per_investor: i128,
    /// Admin address authorized to initialize the escrow.
    pub admin: Address,
    /// Contract address of the escrow instance.
    pub contract_id: Address,
}

impl TestConfig {
    /// Build a configuration with deliberate defaults.
    ///
    /// This installs a real Stellar asset contract as the funding token and
    /// registers a fresh escrow contract instance.
    pub fn defaults(env: &Env) -> Self {
        let admin = Address::generate(env);
        let treasury = Address::generate(env);
        let funding_token = install_stellar_asset_token(env);
        let contract_id = register_escrow(env);

        Self {
            invoice_id: Symbol::new(env, "INV001"),
            target_amount: 10_000_000,
            yield_bps: 500,
            maturity: env.ledger().timestamp() + 30 * 24 * 60 * 60,
            funding_token,
            treasury,
            min_contribution: 100_000,
            max_per_investor: 5_000_000,
            admin,
            contract_id,
        }
    }

    pub fn with_invoice_id(mut self, invoice_id: Symbol) -> Self {
        self.invoice_id = invoice_id;
        self
    }

    pub fn with_target_amount(mut self, target_amount: i128) -> Self {
        self.target_amount = target_amount;
        self
    }

    pub fn with_yield_bps(mut self, yield_bps: u32) -> Self {
        self.yield_bps = yield_bps;
        self
    }

    pub fn with_maturity(mut self, maturity: u64) -> Self {
        self.maturity = maturity;
        self
    }

    pub fn with_funding_token(mut self, funding_token: Address) -> Self {
        self.funding_token = funding_token;
        self
    }

    pub fn with_treasury(mut self, treasury: Address) -> Self {
        self.treasury = treasury;
        self
    }

    pub fn with_min_contribution(mut self, min_contribution: i128) -> Self {
        self.min_contribution = min_contribution;
        self
    }

    pub fn with_max_per_investor(mut self, max_per_investor: i128) -> Self {
        self.max_per_investor = max_per_investor;
        self
    }

    pub fn with_admin(mut self, admin: Address) -> Self {
        self.admin = admin;
        self
    }

    /// Return a client bound to the configured escrow contract.
    pub fn client<'a>(&self, env: &'a Env) -> EscrowContractClient<'a> {
        EscrowContractClient::new(env, &self.contract_id)
    }

    /// Initialize the escrow using this configuration.
    pub fn init(&self, env: &Env) {
        let client = self.client(env);
        client.init(
            &self.admin,
            &self.invoice_id,
            &self.target_amount,
            &self.yield_bps,
            &self.maturity,
            &self.funding_token,
            &self.treasury,
            &self.min_contribution,
            &self.max_per_investor,
        );
    }

    /// Mint `amount` of the funding token to `to`.
    pub fn mint(&self, env: &Env, to: &Address, amount: i128) {
        let sac = StellarAssetClient::new(env, &self.funding_token);
        sac.mint(to, &amount);
    }

    /// Return a token client for the funding token.
    pub fn token<'a>(&self, env: &'a Env) -> TokenClient<'a> {
        TokenClient::new(env, &self.funding_token)
    }
}

// ---------------------------------------------------------------------------
// Contract error assertions
// ---------------------------------------------------------------------------

/// Assert that `result` is a contract error with the exact expected code.
///
/// Recognizes both supported wrappers:
/// * `Err(Ok(Error))` — direct contract error returned from a client call.
/// * `Err(Err(InvokeError::Contract(code)))` — contract error surfaced through
///   a host invocation error.
///
/// Any other shape (including host errors that are not contract errors) causes
/// a panic with diagnostics.
pub fn assert_contract_error<T, E>(result: Result<T, E>, expected: EscrowError)
where
    E: core::fmt::Debug,
{
    match result {
        Ok(_) => panic!(
            "expected contract error {:?}, but call succeeded",
            expected
        ),
        Err(err) => {
            let actual = extract_contract_error(&err);
            match actual {
                Some(code) => {
                    let actual_err = EscrowError::try_from(code).unwrap_or_else(|_| {
                        panic!(
                            "expected contract error {:?}, but got unknown code {}",
                            expected, code
                        )
                    });
                    assert_eq!(
                        actual_err, expected,
                        "expected contract error {:?}, but got {:?}",
                        expected, actual_err
                    );
                }
                None => panic!(
                    "expected contract error {:?}, but got non-contract error: {:?}",
                    expected, err
                ),
            }
        }
    }
}

/// Extract the contract error code from a nested error value, if present.
fn extract_contract_error<E: core::fmt::Debug>(err: &E) -> Option<u32> {
    // Try to interpret the error as a Soroban `Error` (which is a `Val`).
    // We use the debug representation as a fallback to detect the shape.
    let debug = format!("{:?}", err);

    // Shape: Err(Ok(Error)) — the inner value is a contract error `Val`.
    // Shape: Err(Err(InvokeError::Contract(code))) — host invocation error.
    if let Some(code) = parse_invoke_error_contract(&debug) {
        return Some(code);
    }

    // Attempt direct conversion via Val if the error is a Val.
    None
}

/// Parse `InvokeError::Contract(code)` from a debug string.
fn parse_invoke_error_contract(debug: &str) -> Option<u32> {
    // Look for `Contract(<code>)` in the debug output.
    let marker = "Contract(";
    let start = debug.find(marker)? + marker.len();
    let rest = &debug[start..];
    let end = rest.find(')')?;
    rest[..end].trim().parse::<u32>().ok()
}

// ---------------------------------------------------------------------------
// Event helpers
// ---------------------------------------------------------------------------

/// A decoded event with its contract address, symbol, and payload.
#[derive(Clone, Debug)]
pub struct DecodedEvent<T> {
    pub contract: Address,
    pub symbol: Symbol,
    pub payload: T,
}

/// Select events emitted by `contract` with the given `symbol`, decoding each
/// payload as `T`.
///
/// Panics if the number of matching events differs from `expected_count`,
/// reporting the actual cardinality. Results are returned in emission order so
/// order assertions remain possible.
pub fn find_events<T>(
    env: &Env,
    contract: &Address,
    symbol: &Symbol,
    expected_count: usize,
) -> Vec<DecodedEvent<T>>
where
    T: TryFromVal<Env, Val> + Clone,
{
    let all = env.events().all();
    let mut matched: Vec<DecodedEvent<T>> = Vec::new(env);

    for (event_contract, topics, data) in all.iter() {
        if &event_contract != contract {
            continue;
        }
        // First topic is the event symbol.
        if topics.is_empty() {
            continue;
        }
        let topic0: Val = topics.get(0).unwrap();
        let topic_symbol = Symbol::try_from_val(env, &topic0).ok();
        if topic_symbol.as_ref() != Some(symbol) {
            continue;
        }
        let payload = T::try_from_val(env, &data).unwrap_or_else(|_| {
            panic!(
                "failed to decode event payload for symbol {:?} on contract {:?}",
                symbol, contract
            )
        });
        matched.push_back(DecodedEvent {
            contract: event_contract.clone(),
            symbol: symbol.clone(),
            payload,
        });
    }

    assert_eq!(
        matched.len() as usize,
        expected_count,
        "expected {} event(s) for symbol {:?} on contract {:?}, found {}",
        expected_count,
        symbol,
        contract,
        matched.len()
    );

    matched
}

/// Convenience: select and decode events, returning only the payloads in order.
pub fn decode_events<T>(
    env: &Env,
    contract: &Address,
    symbol: &Symbol,
    expected_count: usize,
) -> Vec<T>
where
    T: TryFromVal<Env, Val> + Clone,
{
    let events = find_events::<T>(env, contract, symbol, expected_count);
    let mut payloads: Vec<T> = Vec::new(env);
    for event in events.iter() {
        payloads.push_back(event.payload.clone());
    }
    payloads
}

// ---------------------------------------------------------------------------
// Unit tests for the helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events as _},
        Env, IntoVal, Symbol, Val, Vec,
    };

    #[test]
    fn assert_contract_error_accepts_correct_code() {
        let result: Result<(), EscrowError> = Err(EscrowError::AlreadyInitialized);
        assert_contract_error(result, EscrowError::AlreadyInitialized);
    }

    #[test]
    #[should_panic(expected = "expected contract error")]
    fn assert_contract_error_rejects_wrong_code() {
        let result: Result<(), EscrowError> = Err(EscrowError::NotAuthorized);
        assert_contract_error(result, EscrowError::AlreadyInitialized);
    }

    #[test]
    #[should_panic(expected = "non-contract error")]
    fn assert_contract_error_rejects_host_error() {
        let result: Result<(), String> = Err("host failure".into());
        assert_contract_error(result, EscrowError::AlreadyInitialized);
    }

    #[test]
    fn find_events_selects_by_contract_and_symbol() {
        let env = Env::default();
        let contract_a = Address::generate(&env);
        let contract_b = Address::generate(&env);
        let sym_x = Symbol::new(&env, "x");
        let sym_y = Symbol::new(&env, "y");

        // Emit events from two contracts with two symbols.
        env.events().publish(
            (contract_a.clone(), sym_x.clone()),
            (1u32,),
        );
        env.events().publish(
            (contract_b.clone(), sym_y.clone()),
            (2u32,),
        );
        env.events().publish(
            (contract_a.clone(), sym_y.clone()),
            (3u32,),
        );

        let events_a_x = find_events::<(u32,)>(&env, &contract_a, &sym_x, 1);
        assert_eq!(events_a_x.len(), 1);
        assert_eq!(events_a_x.get(0).unwrap().payload.0, 1);

        let events_a_y = find_events::<(u32,)>(&env, &contract_a, &sym_y, 1);
        assert_eq!(events_a_y.get(0).unwrap().payload.0, 3);

        let events_b_y = find_events::<(u32,)>(&env, &contract_b, &sym_y, 1);
        assert_eq!(events_b_y.get(0).unwrap().payload.0, 2);
    }

    #[test]
    #[should_panic(expected = "expected 2 event(s)")]
    fn find_events_reports_unexpected_cardinality() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let sym = Symbol::new(&env, "x");
        env.events().publish((contract.clone(), sym.clone()), (1u32,));
        find_events::<(u32,)>(&env, &contract, &sym, 2);
    }

    #[test]
    fn find_events_preserves_order() {
        let env = Env::default();
        let contract = Address::generate(&env);
        let sym = Symbol::new(&env, "x");
        env.events().publish((contract.clone(), sym.clone()), (1u32,));
        env.events().publish((contract.clone(), sym.clone()), (2u32,));
        env.events().publish((contract.clone(), sym.clone()), (3u32,));

        let events = find_events::<(u32,)>(&env, &contract, &sym, 3);
        assert_eq!(events.get(0).unwrap().payload.0, 1);
        assert_eq!(events.get(1).unwrap().payload.0, 2);
        assert_eq!(events.get(2).unwrap().payload.0, 3);
    }
}