//! Shared checked basis-point fee split arithmetic.
//!
//! Both the release and withdraw disbursement routes use [`split_fee`] so that
//! rounding direction and overflow policy cannot diverge between them.
//!
//! Rounding policy: the fee is computed with truncating integer division
//! (`gross * fee_bps / 10_000`), matching the historical behavior of both
//! endpoints. The net amount is derived as `gross - fee`, which guarantees
//! `fee + net == gross` exactly.

use soroban_sdk::Env;

use crate::errors::EscrowError;

/// Basis-point denominator (100% == 10_000 bps).
pub(crate) const BPS_DENOMINATOR: i128 = 10_000;

/// Splits `gross` into `(fee, net)` using `fee_bps` basis points.
///
/// * `fee = gross * fee_bps / 10_000` (truncating division)
/// * `net = gross - fee`
///
/// Guarantees `fee + net == gross` and `fee >= 0`, `net >= 0` for
/// non-negative `gross` and `fee_bps` in `[0, 10_000]`.
///
/// Returns [`EscrowError::InvalidFee`] if `fee_bps` is outside `[0, 10_000]`
/// or `gross` is negative, and [`EscrowError::Overflow`] if the intermediate
/// multiplication overflows `i128`.
pub(crate) fn split_fee(
    _env: &Env,
    gross: i128,
    fee_bps: i64,
) -> Result<(i128, i128), EscrowError> {
    if gross < 0 {
        return Err(EscrowError::InvalidFee);
    }
    if fee_bps < 0 || (fee_bps as i128) > BPS_DENOMINATOR {
        return Err(EscrowError::InvalidFee);
    }

    let fee = gross
        .checked_mul(fee_bps as i128)
        .ok_or(EscrowError::Overflow)?
        .checked_div(BPS_DENOMINATOR)
        .ok_or(EscrowError::Overflow)?;

    // `fee` is in `[0, gross]` because `fee_bps <= 10_000` and division
    // truncates, so this subtraction cannot underflow.
    let net = gross.checked_sub(fee).ok_or(EscrowError::Overflow)?;

    Ok((fee, net))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn env() -> Env {
        Env::default()
    }

    #[test]
    fn zero_bps_yields_no_fee() {
        let (fee, net) = split_fee(&env(), 1_000, 0).unwrap();
        assert_eq!(fee, 0);
        assert_eq!(net, 1_000);
        assert_eq!(fee + net, 1_000);
    }

    #[test]
    fn max_bps_yields_full_fee() {
        let (fee, net) = split_fee(&env(), 1_000, 10_000).unwrap();
        assert_eq!(fee, 1_000);
        assert_eq!(net, 0);
        assert_eq!(fee + net, 1_000);
    }

    #[test]
    fn gross_zero_yields_zero_split() {
        let (fee, net) = split_fee(&env(), 0, 250).unwrap();
        assert_eq!(fee, 0);
        assert_eq!(net, 0);
        assert_eq!(fee + net, 0);
    }

    #[test]
    fn gross_one_truncates_fee_to_zero() {
        let (fee, net) = split_fee(&env(), 1, 250).unwrap();
        assert_eq!(fee, 0);
        assert_eq!(net, 1);
        assert_eq!(fee + net, 1);
    }

    #[test]
    fn gross_one_at_max_bps_yields_full_fee() {
        let (fee, net) = split_fee(&env(), 1, 10_000).unwrap();
        assert_eq!(fee, 1);
        assert_eq!(net, 0);
        assert_eq!(fee + net, 1);
    }

    #[test]
    fn truncating_rounding_policy_preserved() {
        // 333 bps of 100 == 3.33 -> truncates to 3.
        let (fee, net) = split_fee(&env(), 100, 333).unwrap();
        assert_eq!(fee, 3);
        assert_eq!(net, 97);
        assert_eq!(fee + net, 100);

        // 1 bps of 9_999 == 0.9999 -> truncates to 0.
        let (fee, net) = split_fee(&env(), 9_999, 1).unwrap();
        assert_eq!(fee, 0);
        assert_eq!(net, 9_999);
        assert_eq!(fee + net, 9_999);
    }

    #[test]
    fn gross_near_i128_max_does_not_overflow_at_low_bps() {
        let gross = i128::MAX;
        let (fee, net) = split_fee(&env(), gross, 1).unwrap();
        // i128::MAX * 1 does not overflow; division truncates.
        assert_eq!(fee, gross / BPS_DENOMINATOR);
        assert_eq!(net, gross - fee);
        assert_eq!(fee + net, gross);
    }

    #[test]
    fn gross_near_i128_max_overflows_at_high_bps() {
        let gross = i128::MAX;
        // i128::MAX * 10_000 overflows i128.
        let result = split_fee(&env(), gross, 10_000);
        assert_eq!(result, Err(EscrowError::Overflow));
    }

    #[test]
    fn invalid_fee_bps_rejected() {
        assert_eq!(split_fee(&env(), 1_000, -1), Err(EscrowError::InvalidFee));
        assert_eq!(
            split_fee(&env(), 1_000, 10_001),
            Err(EscrowError::InvalidFee)
        );
    }

    #[test]
    fn negative_gross_rejected() {
        assert_eq!(split_fee(&env(), -1, 100), Err(EscrowError::InvalidFee));
    }

    #[test]
    fn conservation_holds_across_tranches() {
        // Simulates cumulative tranche disbursement: splitting each tranche
        // individually must conserve gross == fee + net per tranche.
        let tranches = [1_000i128, 2_500, 333, 7, 0, 99_999];
        for &gross in tranches.iter() {
            for &bps in [0i64, 1, 250, 333, 5_000, 9_999, 10_000].iter() {
                let (fee, net) = split_fee(&env(), gross, bps).unwrap();
                assert_eq!(fee + net, gross, "gross={} bps={}", gross, bps);
                assert!(fee >= 0 && net >= 0);
            }
        }
    }
}