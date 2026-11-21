//! Bounded boundary coverage for yield-tier validation and selection.
//!
//! The production code currently has no explicit upper bound on tier-table
//! length. A 64-entry probe records that gap without creating an unbounded run.
//! It also accepts a first tier with `min_lock_secs == 0`, although selection
//! short-circuits lock zero to the base yield, making that tier unreachable at
//! its exact threshold.

use super::{assert_contract_error, deploy, StarfundEscrowClient, TARGET};
use crate::{DataKey, EscrowError, YieldResolution, YieldTier};
use soroban_sdk::{testutils::Address as _, Address, Env, String, Vec as SorobanVec};

const BOUNDED_TIER_COUNT_PROBE: u32 = 64;

fn tier(min_lock_secs: u64, yield_bps: i64) -> YieldTier {
    YieldTier {
        min_lock_secs,
        yield_bps,
    }
}

fn init_with_tiers<'a>(
    env: &'a Env,
    base_yield_bps: i64,
    tiers: Option<SorobanVec<YieldTier>>,
) -> StarfundEscrowClient<'a> {
    env.mock_all_auths();
    let client = deploy(env);
    client.init(
        &Address::generate(env),
        &String::from_str(env, "YTBOUND"),
        &Address::generate(env),
        &TARGET,
        &base_yield_bps,
        &0u64,
        &Address::generate(env),
        &None,
        &Address::generate(env),
        &tiers,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None);
    client
}

fn assert_init_error(
    env: &Env,
    base_yield_bps: i64,
    tiers: SorobanVec<YieldTier>,
    expected: EscrowError,
) {
    env.mock_all_auths();
    let client = deploy(env);
    let result = client.try_init(
        &Address::generate(env),
        &String::from_str(env, "YTERROR"),
        &Address::generate(env),
        &TARGET,
        &base_yield_bps,
        &0u64,
        &Address::generate(env),
        &None,
        &Address::generate(env),
        &Some(tiers),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,

    );
    assert_contract_error(result, expected);
}

#[test]
fn test_yield_bps_accepts_zero_and_max_boundaries() {
    for yield_bps in [0, 10_000] {
        let env = Env::default();
        let tiers = SorobanVec::from_array(&env, [tier(1, yield_bps)]);
        let client = init_with_tiers(&env, 0, Some(tiers));

        assert_eq!(
            client.get_yield_tiers().get_unchecked(0).yield_bps,
            yield_bps
        );
    }
}

#[test]
fn test_yield_bps_rejects_just_outside_range_with_typed_error() {
    for yield_bps in [-1, 10_001] {
        let env = Env::default();
        let tiers = SorobanVec::from_array(&env, [tier(1, yield_bps)]);

        assert_init_error(&env, 0, tiers, EscrowError::TierYieldOutOfRange);
    }
}

#[test]
fn test_yield_bps_rejects_just_below_base_with_typed_error() {
    let env = Env::default();
    let tiers = SorobanVec::from_array(&env, [tier(1, 799)]);

    assert_init_error(&env, 800, tiers, EscrowError::TierYieldBelowBase);
}

#[test]
fn test_lock_boundaries_accept_zero_and_u64_max() {
    let env = Env::default();
    let tiers = SorobanVec::from_array(&env, [tier(0, 100), tier(u64::MAX, 10_000)]);
    let client = init_with_tiers(&env, 0, Some(tiers));

    let stored = client.get_yield_tiers();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored.get_unchecked(0).min_lock_secs, 0);
    assert_eq!(stored.get_unchecked(1).min_lock_secs, u64::MAX);

    assert_eq!(
        client.preview_yield_tier(&1i128, &0u64),
        YieldResolution {
            effective_yield_bps: 0,
            matched_lock_secs: 0
        }
    );
    assert_eq!(
        client.preview_yield_tier(&1i128, &u64::MAX),
        YieldResolution {
            effective_yield_bps: 10_000,
            matched_lock_secs: u64::MAX
        }
    );
}

#[test]
fn test_lock_order_rejects_equal_and_decreasing_boundaries() {
    for second_lock in [10, 9] {
        let env = Env::default();
        let tiers = SorobanVec::from_array(&env, [tier(10, 100), tier(second_lock, 200)]);

        assert_init_error(&env, 0, tiers, EscrowError::TierLockNotIncreasing);
    }
}

#[test]
fn test_yield_order_rejects_decrease_with_typed_error() {
    let env = Env::default();
    let tiers = SorobanVec::from_array(&env, [tier(1, 500), tier(2, 499)]);

    assert_init_error(&env, 0, tiers, EscrowError::TierYieldNotNonDecreasing);
}

#[test]
fn test_tier_table_accepts_zero_and_single_entry_lengths() {
    let empty_env = Env::default();
    let empty = SorobanVec::new(&empty_env);
    let empty_client = init_with_tiers(&empty_env, 0, Some(empty));
    assert_eq!(empty_client.get_yield_tiers().len(), 0);
    assert!(!empty_env.as_contract(&empty_client.address, || {
        empty_env.storage().instance().has(&DataKey::YieldTierTable)
    }));

    let absent_env = Env::default();
    let absent_client = init_with_tiers(&absent_env, 0, None);
    assert!(!absent_env.as_contract(&absent_client.address, || {
        absent_env
            .storage()
            .instance()
            .has(&DataKey::YieldTierTable)
    }));

    let single_env = Env::default();
    let single = SorobanVec::from_array(&single_env, [tier(1, 0)]);
    let single_client = init_with_tiers(&single_env, 0, Some(single));
    assert_eq!(single_client.get_yield_tiers().len(), 1);
}

#[test]
fn test_tier_table_has_no_explicit_length_cap_bounded_probe() {
    let env = Env::default();
    let mut tiers = SorobanVec::new(&env);
    for index in 0..BOUNDED_TIER_COUNT_PROBE {
        tiers.push_back(tier(u64::from(index) + 1, 800));
    }

    let client = init_with_tiers(&env, 800, Some(tiers));

    assert_eq!(client.get_yield_tiers().len(), BOUNDED_TIER_COUNT_PROBE);
}

#[test]
fn test_preview_selection_uses_bounded_boundary_matrix() {
    let env = Env::default();
    let tiers = SorobanVec::from_array(
        &env,
        [
            tier(1, 100),
            tier(10, 200),
            tier(100, 300),
            tier(u64::MAX, 10_000),
        ],
    );
    let client = init_with_tiers(&env, 0, Some(tiers));

    let cases = [
        (
            0,
            YieldResolution {
                effective_yield_bps: 0,
                matched_lock_secs: 0,
            },
        ),
        (
            1,
            YieldResolution {
                effective_yield_bps: 100,
                matched_lock_secs: 1,
            },
        ),
        (
            2,
            YieldResolution {
                effective_yield_bps: 100,
                matched_lock_secs: 1,
            },
        ),
        (
            9,
            YieldResolution {
                effective_yield_bps: 100,
                matched_lock_secs: 1,
            },
        ),
        (
            10,
            YieldResolution {
                effective_yield_bps: 200,
                matched_lock_secs: 10,
            },
        ),
        (
            11,
            YieldResolution {
                effective_yield_bps: 200,
                matched_lock_secs: 10,
            },
        ),
        (
            99,
            YieldResolution {
                effective_yield_bps: 200,
                matched_lock_secs: 10,
            },
        ),
        (
            100,
            YieldResolution {
                effective_yield_bps: 300,
                matched_lock_secs: 100,
            },
        ),
        (
            101,
            YieldResolution {
                effective_yield_bps: 300,
                matched_lock_secs: 100,
            },
        ),
        (
            u64::MAX - 1,
            YieldResolution {
                effective_yield_bps: 300,
                matched_lock_secs: 100,
            },
        ),
        (
            u64::MAX,
            YieldResolution {
                effective_yield_bps: 10_000,
                matched_lock_secs: u64::MAX,
            },
        ),
    ];

    for (lock, expected) in cases {
        assert_eq!(
            client.preview_yield_tier(&i128::MAX, &lock),
            expected,
            "unexpected tier resolution for lock={lock}"
        );
    }
}

/// Builds a valid tier table from `(min_lock_secs, yield_bps)` pairs, asserting
/// the generated table satisfies the production ordering rules.
fn generated_tiers(env: &Env, pairs: &[(u64, i64)]) -> SorobanVec<YieldTier> {
    let mut tiers = SorobanVec::new(env);
    let mut previous: Option<YieldTier> = None;
    for &(min_lock_secs, yield_bps) in pairs {
        if let Some(prev) = previous {
            assert!(
                min_lock_secs > prev.min_lock_secs,
                "generated tier locks must strictly increase"
            );
            assert!(
                yield_bps >= prev.yield_bps,
                "generated tier yields must be non-decreasing"
            );
        }
        let entry = tier(min_lock_secs, yield_bps);
        tiers.push_back(entry.clone());
        previous = Some(entry);
    }
    tiers
}

/// Funds a first commitment and returns the stored effective yield and the
/// stored claim-not-before lock so they can be compared with the preview.
fn fund_first_commitment(
    client: &StarfundEscrowClient,
    env: &Env,
    amount: i128,
    lock_secs: u64,
) -> (i64, u64) {
    let investor = Address::generate(env);
    let deadline = env.ledger().timestamp() + lock_secs;
    client.fund_with_commitment(&investor, &amount, &deadline);

    let stored_yield = client.get_investor_effective_yield(&investor);
    let stored_lock = client.get_investor_claim_not_before(&investor);
    (stored_yield, stored_lock)
}

#[test]
fn test_first_commitment_matches_preview_for_generated_tier_tables() {
    let tables: [&[(u64, i64)]; 4] = [
        // Exact boundaries with distinct rates.
        &[(1, 100), (10, 200), (100, 300)],
        // Equal rates across several boundaries.
        &[(1, 500), (10, 500), (100, 500)],
        // Single boundary.
        &[(50, 250)],
        // Boundary at u64::MAX.
        &[(1, 100), (u64::MAX, 10_000)],
    ];

    for pairs in tables {
        let env = Env::default();
        let tiers = generated_tiers(&env, pairs);
        let client = init_with_tiers(&env, 0, Some(tiers));

        // Probe exact boundaries, just below, just above, and no-match amounts.
        let mut locks: SorobanVec<u64> = SorobanVec::new(&env);
        locks.push_back(0);
        for &(min_lock_secs, _) in pairs {
            if min_lock_secs > 0 {
                locks.push_back(min_lock_secs - 1);
            }
            locks.push_back(min_lock_secs);
            if min_lock_secs < u64::MAX {
                locks.push_back(min_lock_secs + 1);
            }
        }

        for lock in locks.iter() {
            let preview = client.preview_yield_tier(&1_000i128, &lock);
            let (stored_yield, stored_lock) =
                fund_first_commitment(&client, &env, 1_000, lock);

            assert_eq!(
                stored_yield, preview.effective_yield_bps,
                "stored yield drifted from preview for lock={lock}"
            );
            assert_eq!(
                stored_lock, preview.matched_lock_secs,
                "stored claim lock drifted from preview for lock={lock}"
            );
        }
    }
}

#[test]
fn test_first_commitment_no_match_amount_matches_preview() {
    let env = Env::default();
    let tiers = generated_tiers(&env, &[(10, 200), (100, 300)]);
    let client = init_with_tiers(&env, 0, Some(tiers));

    // Amounts below the first tier threshold fall back to the base yield.
    for amount in [0i128, 1, 9] {
        let preview = client.preview_yield_tier(&amount, &100u64);
        let (stored_yield, stored_lock) =
            fund_first_commitment(&client, &env, amount, 100);

        assert_eq!(stored_yield, preview.effective_yield_bps);
        assert_eq!(stored_lock, preview.matched_lock_secs);
    }
}

#[test]
fn test_invalid_maturity_horizon_consistent_between_preview_and_funding() {
    let env = Env::default();
    let tiers = generated_tiers(&env, &[(10, 200), (100, 300)]);
    let client = init_with_tiers(&env, 0, Some(tiers));

    // A deadline in the past yields a zero horizon; both paths must agree on
    // the resulting resolution (base yield, zero matched lock).
    let now = env.ledger().timestamp();
    let past_deadline = now.saturating_sub(1);
    let horizon = past_deadline.saturating_sub(now);

    let preview = client.preview_yield_tier(&1_000i128, &horizon);
    let investor = Address::generate(&env);
    client.fund_with_commitment(&investor, &1_000i128, &past_deadline);

    assert_eq!(
        client.get_investor_effective_yield(&investor),
        preview.effective_yield_bps
    );
    assert_eq!(
        client.get_investor_claim_not_before(&investor),
        preview.matched_lock_secs
    );
}
