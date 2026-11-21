use crate::keys;
use crate::tests::{assert_contract_error, deploy, TARGET};
use crate::{DataKey, EscrowError, FundingRecord, StarfundEscrowClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token::StellarAssetClient, Address, Env, String};

/// An initialised escrow plus the addresses the tests below need.
///
/// Investor state is seeded straight into contract storage rather than through
/// `fund()`: the funding path's `require_auth` sequence cannot be exercised in the
/// SDK-25 test host (the escrow's own address cannot self-authorize), which is a
/// pre-existing upstream defect unrelated to the views and accounting under test
/// here. Everything exercised below — `get_funding_records`, `release`, and
/// `get_distributed_principal` — is a read/accounting path that does not depend on
/// how the contributions were recorded.
struct Fixture<'a> {
    client: StarfundEscrowClient<'a>,
    contract: Address,
}

/// Register a real SEP-41 test token and initialise an escrow against it.
fn init_escrow<'a>(env: &'a Env) -> Fixture<'a> {
    let sac = env.register_stellar_asset_contract_v2(Address::generate(env));
    let token = sac.address();
    let sac_admin = StellarAssetClient::new(env, &token);

    let client = deploy(env);
    let contract = client.address.clone();
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    let treasury = Address::generate(env);

    client.init(
        &admin,
        &String::from_str(env, "INV001"),
        &sme,
        &TARGET,
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

    // Give the escrow the balance it will disburse, so `release`/`withdraw` can transfer.
    sac_admin.mint(&contract, &TARGET);

    Fixture { client, contract }
}

/// Record `investors` as funded participants with the given contributions.
///
/// `contributions[i]` is the principal credited to `investors[i]`; the escrow's
/// `funded_amount` and `InvestorIndex` are updated to match so the view under test
/// sees a realistic, self-consistent state.
fn seed_investors(fx: &Fixture<'_>, investors: &[Address], contributions: &[i128]) {
    let env = fx.client.env.clone();
    let mut total = 0i128;
    for (inv, amount) in investors.iter().zip(contributions.iter()) {
        total += *amount;
        env.as_contract(&fx.contract, || {
            env.storage()
                .persistent()
                .set(&DataKey::InvestorContribution(inv.clone()), amount);
        });
    }

    env.as_contract(&fx.contract, || {
        let mut escrow: crate::InvoiceEscrow =
            env.storage().instance().get(&DataKey::Escrow).unwrap();
        escrow.funded_amount = total;
        // Status 1 (funded) is the state `release` requires.
        if total >= escrow.funding_target {
            escrow.status = 1;
        }
        env.storage().instance().set(&DataKey::Escrow, &escrow);

        let mut index: soroban_sdk::Vec<Address> = env
            .storage()
            .instance()
            .get(&keys::investor_index())
            .unwrap_or_else(|| soroban_sdk::Vec::new(&env));
        for inv in investors.iter() {
            index.push_back(inv.clone());
        }
        env.storage()
            .instance()
            .set(&keys::investor_index(), &index);
        env.storage()
            .instance()
            .set(&DataKey::UniqueFunderCount, &(investors.len() as u32));
    });
}

// ── #122: typed struct return ────────────────────────────────────────────────

/// `get_funding_records` returns `FundingRecord` structs, so callers read named fields
/// (`record.investor`, `record.contribution`) instead of positional tuple indices.
#[test]
fn funding_records_expose_named_struct_fields() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    seed_investors(
        &fx,
        &[a.clone(), b.clone(), c.clone()],
        &[1_000, 1_000, 1_000],
    );
    let client = &fx.client;

    let page = client.get_funding_records(&0u32, &10u32);

    assert_eq!(page.len(), 3);
    // Older positions without the additive historical-total key fall back to the live balance.
    assert_eq!(client.get_total_deposited(&a), 1_000i128);
    for record in page.iter() {
        let as_struct = FundingRecord {
            investor: record.investor.clone(),
            contribution: record.contribution,
        };
        assert!(as_struct.contribution > 0);
    }

    // Each of our investors is discoverable by address, with the exact contribution recorded.
    for inv in [&a, &b, &c] {
        let found = page
            .iter()
            .find(|r| r.investor == *inv)
            .expect("investor present in funding records");
        assert_eq!(found.contribution, 1_000i128);
    }
}

/// Pagination still returns the full active set across successive pages.
#[test]
fn funding_records_pagination_returns_all_active_investors() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    let invs: Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
    seed_investors(&fx, &invs, &[1_000, 1_000, 1_000, 1_000]);
    let client = &fx.client;

    let page0 = client.get_funding_records(&0u32, &2u32);
    let page1 = client.get_funding_records(&2u32, &2u32);

    assert_eq!(page0.len(), 2);
    assert!(page1.len() >= 1);

    let mut seen = 0usize;
    for page in [&page0, &page1] {
        for record in page.iter() {
            assert!(record.contribution > 0);
            seen += 1;
        }
    }
    // All four seeded investors appear across the two pages.
    assert_eq!(seen, 4);
}

// ── #123: zero-balance filtering ────────────────────────────────────────────

/// An investor who withdraws all of their principal keeps an `InvestorIndex` slot but must
/// not surface in `get_funding_records` with a zero contribution.
#[test]
fn funding_records_exclude_investor_who_unfunded_to_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    let quitter = Address::generate(&env);
    let stayer = Address::generate(&env);
    seed_investors(&fx, &[quitter.clone(), stayer.clone()], &[1_000, 1_000]);
    let client = &fx.client;

    // Precondition: the investor is listed while funded.
    assert!(client
        .get_funding_records(&0u32, &50u32)
        .iter()
        .any(|r| r.investor == quitter));

    // Fully withdraw the investor's principal.
    client.unfund(&quitter, &1_000i128);
    assert_eq!(client.get_contribution(&quitter), 0i128);

    let page = client.get_funding_records(&0u32, &50u32);

    // The zero-balance slot is gone from the view...
    assert!(!page.iter().any(|r| r.investor == quitter));
    // ...and no returned record carries a zero contribution.
    for record in page.iter() {
        assert!(
            record.contribution > 0,
            "zero-contribution record surfaced for {:?}",
            record.investor
        );
    }
    // The other investor is still reported.
    assert!(page.iter().any(|r| r.investor == stayer));
}

/// A partially unfunded investor remains visible with their reduced (but positive) balance.
#[test]
fn funding_records_keep_partially_unfunded_investor_with_reduced_balance() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    let inv = Address::generate(&env);
    seed_investors(&fx, &[inv.clone()], &[1_000]);
    let client = &fx.client;

    client.unfund(&inv, &400i128);

    let page = client.get_funding_records(&0u32, &50u32);
    let found = page
        .iter()
        .find(|r| r.investor == inv)
        .expect("partially unfunded investor still listed");
    assert_eq!(found.contribution, 600i128);
}

// ── #124: DistributedPrincipal advances on release ───────────────────────────

/// After a partial `release`, `DistributedPrincipal` equals the released amount — the
/// accounting `sweep_terminal_dust` uses for its liability floor.
#[test]
fn partial_release_advances_distributed_principal() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    seed_investors(&fx, &[Address::generate(&env)], &[TARGET]);
    let client = &fx.client;

    assert_eq!(client.get_distributed_principal(), 0i128);

    let partial = TARGET / 4;
    client.release(&partial);

    assert_eq!(client.get_distributed_principal(), partial);
}

/// A final `release` of the full remaining obligation leaves `DistributedPrincipal` equal to
/// the full `funded_amount`.
#[test]
fn final_release_advances_distributed_principal_to_full_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    seed_investors(&fx, &[Address::generate(&env)], &[TARGET]);
    let client = &fx.client;

    client.release(&TARGET);

    assert_eq!(client.get_escrow().status, 3u32);
    assert_eq!(client.get_distributed_principal(), TARGET);
}

/// Successive partial releases accumulate: the total equals the sum of the released amounts.
#[test]
fn successive_partial_releases_accumulate_distributed_principal() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    seed_investors(&fx, &[Address::generate(&env)], &[TARGET]);
    let client = &fx.client;

    let p1 = TARGET / 2;
    client.release(&p1);
    assert_eq!(client.get_distributed_principal(), p1);

    let p2 = TARGET - p1;
    client.release(&p2);

    assert_eq!(client.get_distributed_principal(), TARGET);
    // Both paths agree: reconciliation reports no outstanding liability.
    let view = client.get_reconciliation();
    assert_eq!(view.outstanding_liability, 0i128);
}

// ── #125: raise_min_contribution_floor upper bound ───────────────────────────

/// Initialise an escrow with an open floor and return the client plus its addresses.
fn open_floor_escrow(env: &Env, initial_floor: i128) -> (crate::StarfundEscrowClient<'_>, Address) {
    env.mock_all_auths();
    let client = crate::tests::deploy(env);
    let admin = Address::generate(env);
    let sme = Address::generate(env);
    client.init(
        &admin,
        &String::from_str(env, "FLOOR_RAISE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &Address::generate(env),
        &None,
        &Address::generate(env),
        &None,
        &Some(initial_floor),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None);
    (client, admin)
}

/// Raising the floor to a value at or below the funding target succeeds and persists.
#[test]
fn raise_min_contribution_floor_within_target_succeeds() {
    let env = Env::default();
    let (client, _admin) = open_floor_escrow(&env, 1_000i128);

    let new_floor = client.raise_min_contribution_floor(&2_000i128);
    assert_eq!(new_floor, 2_000i128);
    assert_eq!(client.get_min_contribution_floor(), 2_000i128);
}

/// The acceptance criterion for #125: a floor above `funding_target` reverts with
/// `MinContributionExceedsAmount`, matching the bound enforced at `init`.
#[test]
fn raise_min_contribution_floor_above_funding_target_reverts() {
    let env = Env::default();
    let (client, _admin) = open_floor_escrow(&env, 1_000i128);

    assert_contract_error(
        client.try_raise_min_contribution_floor(&(TARGET + 1)),
        EscrowError::MinContributionExceedsAmount,
    );

    // State is unchanged after the rejected call.
    assert_eq!(client.get_min_contribution_floor(), 1_000i128);
}

/// A floor exactly equal to the funding target is the boundary and must be accepted —
/// a single investor can still reach the target exactly.
#[test]
fn raise_min_contribution_floor_equal_to_target_accepted() {
    let env = Env::default();
    let (client, _admin) = open_floor_escrow(&env, 1_000i128);

    assert_eq!(client.raise_min_contribution_floor(&TARGET), TARGET);
    assert_eq!(client.get_min_contribution_floor(), TARGET);
}

/// A non-raising value is rejected — the setter only raises.
#[test]
fn raise_min_contribution_floor_requires_strict_increase() {
    let env = Env::default();
    let (client, _admin) = open_floor_escrow(&env, 5_000i128);

    assert_contract_error(
        client.try_raise_min_contribution_floor(&5_000i128),
        EscrowError::NewFloorNotHigher,
    );
    assert_contract_error(
        client.try_raise_min_contribution_floor(&1_000i128),
        EscrowError::NewFloorNotHigher,
    );
}

/// A non-positive floor is rejected.
#[test]
fn raise_min_contribution_floor_rejects_non_positive() {
    let env = Env::default();
    let (client, _admin) = open_floor_escrow(&env, 1_000i128);

    assert_contract_error(
        client.try_raise_min_contribution_floor(&0i128),
        EscrowError::NewFloorNotPositive,
    );
}

/// The setter is admin-only: it cannot run once the escrow has left the open state.
#[test]
fn raise_min_contribution_floor_rejects_when_not_open() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    seed_investors(&fx, &[Address::generate(&env)], &[TARGET]);
    let client = &fx.client;
    assert_eq!(client.get_escrow().status, 1u32);

    assert_contract_error(
        client.try_raise_min_contribution_floor(&1_000i128),
        EscrowError::FloorRaiseNotOpen,
    );
}

// ── #408: cumulative deposit provenance ─────────────────────────────────────

/// A legacy position with only a contribution (no cumulative key) is reported as a
/// fallback lower bound, while an investor with a cumulative key is reported as recorded —
/// even when both numeric totals are equal.
#[test]
fn total_deposited_provenance_distinguishes_fallback_from_recorded() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    let legacy = Address::generate(&env);
    let recorded = Address::generate(&env);
    seed_investors(&fx, &[legacy.clone(), recorded.clone()], &[1_000, 1_000]);
    let client = &fx.client;

    // Give the recorded investor an explicit cumulative key equal to its contribution.
    env.as_contract(&fx.contract, || {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposited(recorded.clone()), &1_000i128);
    });

    // The existing numeric getter is unchanged and reports equal values.
    assert_eq!(client.get_total_deposited(&legacy), 1_000i128);
    assert_eq!(client.get_total_deposited(&recorded), 1_000i128);

    let legacy_view = client.get_total_deposited_with_provenance(&legacy);
    let recorded_view = client.get_total_deposited_with_provenance(&recorded);

    assert_eq!(legacy_view.total, 1_000i128);
    assert_eq!(recorded_view.total, 1_000i128);
    assert_ne!(legacy_view.provenance, recorded_view.provenance);
    assert_eq!(
        legacy_view.provenance,
        crate::DepositProvenance::Fallback
    );
    assert_eq!(
        recorded_view.provenance,
        crate::DepositProvenance::Recorded
    );
}

/// An account with no position at all is explicitly identified as unknown.
#[test]
fn total_deposited_provenance_identifies_unknown_account() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    seed_investors(&fx, &[Address::generate(&env)], &[1_000]);
    let client = &fx.client;

    let stranger = Address::generate(&env);
    let view = client.get_total_deposited_with_provenance(&stranger);

    assert_eq!(view.total, 0i128);
    assert_eq!(view.provenance, crate::DepositProvenance::Unknown);
}

// ── #409: explicit funding record pagination continuation ────────────────────

/// Seed an `InvestorIndex` with explicit slots, including zero-balance (sparse) entries.
///
/// `slots[i]` is the address at index `i`; a slot whose contribution is zero is a dead
/// entry that `get_funding_records` filters out but which still occupies index space.
fn seed_sparse_index(fx: &Fixture<'_>, slots: &[Address], contributions: &[i128]) {
    let env = fx.client.env.clone();
    let mut total = 0i128;
    for (inv, amount) in slots.iter().zip(contributions.iter()) {
        if *amount > 0 {
            total += *amount;
            env.as_contract(&fx.contract, || {
                env.storage()
                    .persistent()
                    .set(&DataKey::InvestorContribution(inv.clone()), amount);
            });
        }
    }

    env.as_contract(&fx.contract, || {
        let mut escrow: crate::InvoiceEscrow =
            env.storage().instance().get(&DataKey::Escrow).unwrap();
        escrow.funded_amount = total;
        if total >= escrow.funding_target {
            escrow.status = 1;
        }
        env.storage().instance().set(&DataKey::Escrow, &escrow);

        let mut index: soroban_sdk::Vec<Address> =
            soroban_sdk::Vec::new(&env);
        for inv in slots.iter() {
            index.push_back(inv.clone());
        }
        env.storage()
            .instance()
            .set(&keys::investor_index(), &index);
        env.storage()
            .instance()
            .set(&DataKey::UniqueFunderCount, &(slots.len() as u32));
    });
}

/// A wholly empty filtered page still carries a continuation, and concatenating pages
/// visits every live record exactly once.
#[test]
fn funding_records_page_carries_continuation_across_empty_middle_page() {
    let env = Env::default();
    env.mock_all_auths();
    let fx = init_escrow(&env);
    let client = &fx.client;

    // Slots: live, dead, dead, live, dead, live — a wholly empty middle page of size 2.
    let live_a = Address::generate(&env);
    let dead_1 = Address::generate(&env);
    let dead_2 = Address::generate(&env);
    let live_b = Address::generate(&env);
    let dead_3 = Address::generate(&env);
    let live_c = Address::generate(&env);
    seed_sparse_index(
        &fx,
        &[
            live_a.clone(),
            dead_1,
            dead_2,
            live_b.clone(),
            dead_3,
            live_c.clone(),
        ],
        &[1_000, 0, 0, 1_000, 0, 1_000],
    );

    // First page: one live record, continuation present.
    let page0 = client.get_funding_records_page(&0u32, &2u32);
    assert_eq!(page0.records.len(), 1);
    assert_eq!(page0.records.get(0).unwrap().investor, live_a);
    assert!(!page0.done);
    assert!(page0.next_offset > 0u32);

    // Second page: wholly empty, but must still carry a continuation.
    let page1 = client.get_funding_records_page(&page0.next_offset, &2u32);
    assert_eq!(page1.records.len(), 0);
    assert!(!page1.done);
    assert!(page1.next_offset > page0.next_offset);

    // Walk to completion, collecting every live record exactly once.
    let mut seen: Vec<Address> = Vec::new();
    let mut offset = page1.next_offset;
    let mut guard = 0u32;
    loop {
        let page = client.get_funding_records_page(&offset, &2u32);
        for record in page.records.iter() {
            seen.push(record.investor.clone());
        }
        if page.done {
            break;
        }
        assert!(page.next_offset > offset);
        offset = page.next_offset;
        guard += 1;
        assert!(guard < 100, "pagination did not terminate");
    }

    // The first page's record plus the tail must cover every live investor once.
    let mut all: Vec<Address> = Vec::new();
    all.push_back(live_a.clone());
    for inv in seen.iter() {
        all.push_back(inv.clone());
    }
    assert_eq!(all.len(), 3);
    for live in [&live_a, &live_b, &live_c] {
        let count = all.iter().filter(|i| *i == live).count();
        assert_eq!(count, 1, "live record visited exactly once");
    }

    // The legacy getter still returns the same live set.
    let legacy = client.get_funding_records(&0u32, &50u32);
    assert_eq!(legacy.len(), 3);
}
