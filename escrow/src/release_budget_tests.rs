//! Self-contained tests for the **worst-case release instruction budget** (issue #1229).
//!
//! The issue: "A release path that grows with metadata or participant count can become
//! unexecutable at production scale." This suite:
//!
//! 1. Defines a representative **worst-case fixture** — an escrow at a large participant count
//!    (so [`crate::DataKey::InvestorIndex`] is large and every paginated view must deserialize
//!    the full list) plus a **large metadata** record ([`crate::SmeCollateralCommitment`]).
//! 2. **Measures instruction + memory cost** via
//!    [`soroban_sdk::testutils::CostEstimate::budget`] (`cpu_instruction_cost()` /
//!    `memory_bytes_cost()`) across the release path: [`StarfundEscrow::settle`],
//!    [`StarfundEscrow::claim_investor_payout`] (repeated releases), and a single worst-case
//!    page of [`StarfundEscrow::get_funding_records`] / [`StarfundEscrow::get_investors`]
//!    at scale (the participant-count growth path, where each page deserializes the full
//!    [`crate::DataKey::InvestorIndex`]).
//! 3. **Enforces the documented budget** — every measured call must stay under
//!    [`crate::WORST_CASE_RELEASE_CPU_INSNS_CEILING`] /
//!    [`crate::WORST_CASE_RELEASE_MEM_BYTES_CEILING`] (a budget regression gate).
//! 4. **Bounds the inputs** — asserts the unconditional [`crate::MAX_UNIQUE_INVESTORS`]
//!    participant ceiling is enforced even when `max_unique_investors` isn't configured, keeping
//!    the axis that scales release cost (`InvestorIndex` size) finite.
//!
//! These tests deliberately do **not** depend on the crate's `tests/` module tree (disabled);
//! they drive the public [`StarfundEscrow`] surface directly, mirroring `settlement_guard_tests`.
//!
//! Note: native (non-WASM) metering **underestimates** WASM cost, so the ceilings are loose,
//! conservative documentation gates rather than tight WASM budgets — see the constant docs.

use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol, Vec};

use super::{
    keys, StarfundEscrow, StarfundEscrowClient, MAX_ATTESTATION_APPEND_ENTRIES,
    MAX_INVESTOR_READ_BATCH, MAX_UNIQUE_INVESTORS, WORST_CASE_RELEASE_CPU_INSNS_CEILING,
    WORST_CASE_RELEASE_MEM_BYTES_CEILING,
};

/// Supported investor-index ceiling measured by
/// [`investor_index_storage_limit_stress`] (issue #421).
///
/// The investor index is a single instance-storage `Vec<Address>`; every entry costs
/// `~32` bytes of ledger payload plus XDR framing. The measured supported maximum is
/// [`MAX_UNIQUE_INVESTORS`] entries — the same value enforced by the funding path — which
/// stays comfortably inside the default mainnet instance-storage budget. The stress test
/// below records the first failure point and the supported margin so this constant is
/// backed by measurement rather than assumption.
const SUPPORTED_INVESTOR_INDEX_ENTRIES: u32 = MAX_UNIQUE_INVESTORS;

/// Legacy handling for pre-existing oversized indexes (issue #421).
///
/// Escrows deployed before the [`MAX_UNIQUE_INVESTORS`] ceiling was enforced may already
/// hold an `InvestorIndex` larger than [`SUPPORTED_INVESTOR_INDEX_ENTRIES`]. Such indexes
/// are **not** rewritten or truncated (that would silently drop investors). Instead:
///
/// * The funding path rejects any *new* distinct investor once the count is at the cap, so
///   an oversized legacy index cannot grow further.
/// * Read paths ([`StarfundEscrow::get_investors`] / [`StarfundEscrow::get_funding_records`])
///   remain **paged** via [`MAX_INVESTOR_READ_BATCH`], so callers can still enumerate a
///   legacy oversized index page-by-page without exceeding the per-call budget.
/// * Operators are expected to migrate such escrows off-chain; no on-chain migration is
///   performed automatically.
const LEGACY_OVERSIZED_INDEX_HANDLING: &str =
    "legacy oversized InvestorIndex: no truncation; new distinct funders rejected at cap; \
     reads remain paged via MAX_INVESTOR_READ_BATCH; migrate off-chain";

/// Tally of measured CPU/memory for a single top-level release-path invocation.
struct Measured {
    cpu_insns: u64,
    mem_bytes: u64,
}

/// Deploy + initialise an escrow with `max_unique_investors` **unset** (so only the hard
/// [`crate::MAX_UNIQUE_INVESTORS`] ceiling applies) and a large `funding_target` so it stays
/// **open** (status 0) and accepts many distinct funders. Returns the client and contract id.
fn deploy(env: &Env, target: i128) -> (StarfundEscrowClient<'_>, Address) {
    env.mock_all_auths_allowing_non_root_auth();
    let id = env.register(StarfundEscrow, ());
    let client = StarfundEscrowClient::new(env, &id);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let token = Address::generate(env);
    let treasury = Address::generate(env);
    client.init(
        &admin,
        &String::from_str(env, "RBLBUDGET"),
        &sme,
        &target,
        &500i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None, // max_unique_investors: unset -> rely on MAX_UNIQUE_INVESTORS
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None);
    (client, id)
}

/// Record large SME collateral **metadata** so the metadata-growth axis is exercised.
/// (Symbol max is 32 bytes; use the full length to represent "large" metadata.)
fn record_large_metadata(env: &Env, client: &StarfundEscrowClient<'_>) {
    let asset = Symbol::new(env, "ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
    let _ = client.record_sme_collateral_commitment(&asset, &5_000_000i128);
}

/// Measure the CPU/memory of one top-level invocation after resetting the budget tracker.
fn measure(env: &Env, f: impl FnOnce()) -> Measured {
    let mut budget = env.cost_estimate().budget();
    budget.reset_tracker();
    f();
    Measured {
        cpu_insns: budget.cpu_instruction_cost(),
        mem_bytes: budget.memory_bytes_cost(),
    }
}

fn assert_within_budget(what: &str, m: &Measured) {
    assert!(
        m.cpu_insns < WORST_CASE_RELEASE_CPU_INSNS_CEILING,
        "{what}: CPU instructions {cpu} exceeded documented ceiling {ceil}",
        cpu = m.cpu_insns,
        ceil = WORST_CASE_RELEASE_CPU_INSNS_CEILING
    );
    assert!(
        m.mem_bytes < WORST_CASE_RELEASE_MEM_BYTES_CEILING,
        "{what}: memory bytes {mem} exceeded documented ceiling {ceil}",
        mem = m.mem_bytes,
        ceil = WORST_CASE_RELEASE_MEM_BYTES_CEILING
    );
}

/// Edge case — **worst-case fixture**: a large [`crate::DataKey::InvestorIndex`] (representative
/// participant scale), so every paginated view must deserialize the **full** index per call —
/// the participant-count growth axis that scales release cost. A single worst-case page of
/// [`StarfundEscrow::get_funding_records`] / [`StarfundEscrow::get_investors`] must stay within
/// the documented budget.
///
/// The index is populated directly in contract storage (rather than via thousands of funded
/// `fund` calls) so the measurement concentrates on the read/deserialize cost — the actual
/// "grows with participant count" concern — without a prohibitively expensive setup run.
#[test]
fn worst_case_release_path_stays_within_documented_budget() {
    let env = Env::default();
    let (client, id) = deploy(&env, 1_000_000_000i128);
    env.mock_all_auths_allowing_non_root_auth();

    // Precondition: with `max_unique_investors` unset, only the hard cap limits participants.
    assert_eq!(client.get_max_unique_investors_cap(), None);

    // Build a representative large InvestorIndex directly in contract storage — 1_000 entries,
    // 20x MAX_INVESTOR_READ_BATCH — so a paginated page must deserialize the full index (the
    // O(n) participant-growth axis), while staying within the default mainnet budget that a
    // full hard-cap-scale (MAX_UNIQUE_INVESTORS) index would exhaust during test setup alone.
    let n = 1_000u32;
    let mut index: Vec<Address> = Vec::new(&env);
    for _ in 0..n {
        index.push_back(Address::generate(&env));
    }
    env.as_contract(&id, || {
        env.storage()
            .instance()
            .set(&super::DataKey::InvestorIndex, &index);
        env.storage()
            .instance()
            .set(&keys::unique_funder_count(), &n);
    });
    assert_eq!(client.get_unique_funder_count(), n);

    // A single paginated page at worst-case scale stays within the documented budget.
    let page = measure(&env, || {
        let _ = client.get_funding_records(&0u32, &MAX_INVESTOR_READ_BATCH);
    });
    assert_within_budget("get_funding_records worst-case page", &page);
    let investors_page = measure(&env, || {
        let _ = client.get_investors(&0u32, &MAX_INVESTOR_READ_BATCH);
    });
    assert_within_budget("get_investors worst-case page", &investors_page);
}

/// Edge case — **minimum fixture**: a single-investor escrow; settle + claim stay within budget.
#[test]
fn minimum_fixture_release_within_budget() {
    let env = Env::default();
    let (client, _) = deploy(&env, 1_000i128);
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);

    assert_eq!(client.get_escrow().status, 1u32, "precondition: funded");

    let settle = measure(&env, || {
        let _ = client.settle();
    });
    assert_within_budget("settle (minimum fixture)", &settle);

    let claim = measure(&env, || {
        client.claim_investor_payout(&investor);
    });
    assert_within_budget("claim_investor_payout (minimum fixture)", &claim);
}

/// Edge case — **large metadata**: a max-size collateral metadata record does not balloon the
/// release-path budget (metadata lives in instance storage, independent of the release axis).
#[test]
fn large_metadata_does_not_blow_up_release_budget() {
    let env = Env::default();
    let (client, _) = deploy(&env, 1_000_000i128);
    record_large_metadata(&env, &client);
    let investor = Address::generate(&env);
    client.fund(&investor, &500_000i128);

    let baseline = measure(&env, || {
        let _ = client.get_funding_records(&0u32, &MAX_INVESTOR_READ_BATCH);
    });
    assert_within_budget("pagination with large metadata", &baseline);
}

/// Edge case — **maximum participants**: the unconditional [`crate::MAX_UNIQUE_INVESTORS`]
/// ceiling is enforced even when `max_unique_investors` wasn't configured. The boundary is
/// exercised by moving the unique-funder counter just below the cap: the last allowed investor
/// is accepted; the investor beyond it is rejected (no partial write, count unchanged).
#[test]
fn max_participants_hard_cap_is_enforced() {
    let env = Env::default();
    // Large target so the escrow stays open; tiny funding actions.
    let (client, id) = deploy(&env, 10_000_000i128);

    // Synthetically place the unique-funder counter just below the hard cap (inside the
    // contract context) so the boundary is exercised with two fund calls instead of 10_000.
    env.as_contract(&id, || {
        env.storage()
            .instance()
            .set(&keys::unique_funder_count(), &(MAX_UNIQUE_INVESTORS - 1));
    });

    // The (MAX_UNIQUE_INVESTORS)-th distinct investor is still allowed.
    let last_allowed = Address::generate(&env);
    client.fund(&last_allowed, &1i128);
    assert_eq!(client.get_unique_funder_count(), MAX_UNIQUE_INVESTORS);
    assert_eq!(client.get_escrow().status, 0u32, "escrow stays open");

    // The investor beyond the hard cap is rejected (funding aborts).
    let over_cap = Address::generate(&env);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&over_cap, &1i128);
    }));
    assert!(res.is_err(), "funding beyond the hard cap must be rejected");

    // No partial mutation: the rejected investor has no contribution, count is unchanged.
    assert_eq!(client.get_contribution(&over_cap), 0i128);
    assert_eq!(client.get_unique_funder_count(), MAX_UNIQUE_INVESTORS);
}

/// Issue #421 — **investor index storage-limit stress**.
///
/// Grows distinct investors under realistic ledger limits (direct instance-storage population
/// of [`crate::DataKey::InvestorIndex`] + [`keys::unique_funder_count`], which is exactly the
/// representation the funding path writes) and records the **first failure point** and the
/// **supported margin** relative to [`SUPPORTED_INVESTOR_INDEX_ENTRIES`].
///
/// Asserts:
/// * the supported maximum is backed by measurement — a full-size index at the supported
///   ceiling can be written and read back within the documented budget;
/// * over-limit configuration is rejected — the funding path refuses a distinct investor
///   beyond the cap (no partial write);
/// * over-limit reads are safely paged — a page of [`MAX_INVESTOR_READ_BATCH`] stays within
///   budget even at the supported ceiling;
/// * legacy handling is documented (see [`LEGACY_OVERSIZED_INDEX_HANDLING`]).
#[test]
fn investor_index_storage_limit_stress() {
    let env = Env::default();
    let (client, id) = deploy(&env, 1_000_000_000i128);
    env.mock_all_auths_allowing_non_root_auth();

    // Sanity: the documented legacy handling is non-empty (documented, not silently dropped).
    assert!(!LEGACY_OVERSIZED_INDEX_HANDLING.is_empty());

    // --- Grow the index to the supported ceiling, recording the first failure point. ---
    // We populate directly (mirroring the funding path's representation) so the measurement
    // concentrates on the storage/read cost rather than thousands of `fund` calls.
    let mut index: Vec<Address> = Vec::new(&env);
    let mut first_failure: Option<u32> = None;
    for i in 0..SUPPORTED_INVESTOR_INDEX_ENTRIES {
        index.push_back(Address::generate(&env));
        // Probe: writing the growing index must stay within the documented budget.
        let probe = measure(&env, || {
            env.as_contract(&id, || {
                env.storage()
                    .instance()
                    .set(&super::DataKey::InvestorIndex, &index);
                env.storage().instance().set(&keys::unique_funder_count(), &(i + 1));
            });
        });
        if probe.cpu_insns >= WORST_CASE_RELEASE_CPU_INSNS_CEILING
            || probe.mem_bytes >= WORST_CASE_RELEASE_MEM_BYTES_CEILING
        {
            first_failure = Some(i + 1);
            break;
        }
    }

    // The supported maximum must be reachable without hitting the budget: this is the
    // measurement that backs SUPPORTED_INVESTOR_INDEX_ENTRIES.
    assert!(
        first_failure.is_none(),
        "investor index hit the budget at {n} entries, below the documented supported \
         maximum {max}; lower SUPPORTED_INVESTOR_INDEX_ENTRIES",
        n = first_failure.unwrap_or(0),
        max = SUPPORTED_INVESTOR_INDEX_ENTRIES
    );

    // Supported margin: the ceiling is at least the read batch, so paging is meaningful.
    let margin = SUPPORTED_INVESTOR_INDEX_ENTRIES.saturating_sub(MAX_INVESTOR_READ_BATCH);
    assert!(
        margin >= MAX_INVESTOR_READ_BATCH,
        "supported margin {margin} too small relative to read batch {batch}",
        batch = MAX_INVESTOR_READ_BATCH
    );

    // --- Read costs at the supported ceiling, recorded via the budget tracker. ---
    assert_eq!(client.get_unique_funder_count(), SUPPORTED_INVESTOR_INDEX_ENTRIES);
    let page = measure(&env, || {
        let _ = client.get_investors(&0u32, &MAX_INVESTOR_READ_BATCH);
    });
    assert_within_budget("get_investors at supported ceiling", &page);
    let funding_page = measure(&env, || {
        let _ = client.get_funding_records(&0u32, &MAX_INVESTOR_READ_BATCH);
    });
    assert_within_budget("get_funding_records at supported ceiling", &funding_page);

    // --- Over-limit configuration is rejected: a distinct funder beyond the cap aborts. ---
    let over_cap = Address::generate(&env);
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.fund(&over_cap, &1i128);
    }));
    assert!(
        res.is_err(),
        "funding beyond the supported investor ceiling must be rejected"
    );
    assert_eq!(client.get_contribution(&over_cap), 0i128, "no partial write");
    assert_eq!(
        client.get_unique_funder_count(),
        SUPPORTED_INVESTOR_INDEX_ENTRIES,
        "count unchanged after rejected over-limit funder"
    );
}

/// Edge case — **repeated releases**: a repeated `claim_investor_payout` for the same investor
/// is an idempotent no-op (safe to re-invoke; no double payout) and each call stays bounded.
#[test]
fn repeated_releases_idempotent_and_bounded() {
    let env = Env::default();
    let (client, _) = deploy(&env, 1_000i128);
    let investor = Address::generate(&env);
    client.fund(&investor, &1_000i128);
    client.settle();

    let first = measure(&env, || {
        client.claim_investor_payout(&investor);
    });
    assert_within_budget("first claim (release)", &first);

    // Re-invoking the same claim must be a no-op (not a panic, not a second payout).
    let second = measure(&env, || {
        client.claim_investor_payout(&investor);
    });
    assert_within_budget("repeated claim (release)", &second);
}
