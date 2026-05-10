pub(crate) fn choose_target_sized_token_amount<F>(
    available_raw: u64,
    target_lamports: u64,
    mut quote_lamports: F,
) -> Result<u64, String>
where
    F: FnMut(u64) -> Result<u64, String>,
{
    if target_lamports == 0 {
        return Err("sellOutputSol must be greater than zero.".to_string());
    }
    if available_raw == 0 {
        return Err("You have 0 tokens.".to_string());
    }

    let full_quote = quote_lamports(available_raw)?;
    if full_quote == 0 {
        return Err("Sell quote resolved to zero SOL.".to_string());
    }
    if full_quote < target_lamports {
        return Err(unreachable_target_message(target_lamports, full_quote));
    }

    let mut best = Some((available_raw, full_quote));
    let mut low = 1u64;
    let mut high = available_raw.saturating_sub(1);
    let estimate = target_amount_estimate(available_raw, target_lamports, full_quote);
    if estimate < available_raw {
        let quoted = quote_lamports(estimate)?;
        if quoted == 0 {
            low = estimate.saturating_add(1);
        } else {
            best = Some(prefer_better_target_amount(
                best,
                estimate,
                quoted,
                target_lamports,
            ));
            if quoted < target_lamports {
                low = estimate.saturating_add(1);
            } else {
                high = estimate.saturating_sub(1);
            }
        }
    }
    while low <= high {
        let amount = low + (high - low) / 2;
        let quoted = quote_lamports(amount)?;
        if quoted == 0 {
            low = amount.saturating_add(1);
            continue;
        }
        best = Some(prefer_better_target_amount(
            best,
            amount,
            quoted,
            target_lamports,
        ));
        if quoted < target_lamports {
            low = amount.saturating_add(1);
        } else if amount == 0 {
            break;
        } else {
            high = amount - 1;
        }
    }

    let (amount, _) = best.ok_or_else(|| "Sell quote resolved to zero SOL.".to_string())?;
    Ok(amount)
}

pub(crate) const RPC_TARGET_SIZING_MAX_REFINEMENT_PROBES: usize = 16;

pub(crate) fn target_amount_estimate(
    available_raw: u64,
    target_lamports: u64,
    full_quote_lamports: u64,
) -> u64 {
    if available_raw <= 1 || full_quote_lamports == 0 {
        return available_raw.max(1);
    }
    let numerator = u128::from(available_raw).saturating_mul(u128::from(target_lamports));
    let estimate = numerator.saturating_add(u128::from(full_quote_lamports).saturating_sub(1))
        / u128::from(full_quote_lamports);
    estimate.clamp(1, u128::from(available_raw)) as u64
}

pub(crate) fn unreachable_target_message(target_lamports: u64, max_quote_lamports: u64) -> String {
    format!(
        "Not enough tokens to sell for {} SOL. Current balance is quoted for up to {} SOL after fees.",
        format_sol_amount(target_lamports),
        format_sol_amount(max_quote_lamports)
    )
}

fn format_sol_amount(lamports: u64) -> String {
    let whole = lamports / 1_000_000_000;
    let frac = lamports % 1_000_000_000;
    if frac == 0 {
        return whole.to_string();
    }
    let mut frac_text = format!("{frac:09}");
    while frac_text.ends_with('0') {
        frac_text.pop();
    }
    format!("{whole}.{frac_text}")
}

pub(crate) fn net_sol_after_wrapper_fee(gross_lamports: u64) -> Result<u64, String> {
    let fee_bps = u128::from(crate::rollout::wrapper_default_fee_bps());
    let fee =
        ((u128::from(gross_lamports) * fee_bps) / 10_000u128).min(u128::from(u64::MAX)) as u64;
    gross_lamports
        .checked_sub(fee)
        .ok_or_else(|| "Wrapper fee exceeded SOL output quote.".to_string())
}

pub(crate) fn prefer_better_target_amount(
    current: Option<(u64, u64)>,
    amount: u64,
    quoted: u64,
    target_lamports: u64,
) -> (u64, u64) {
    match current {
        Some((best_amount, best_quote))
            if quoted < target_lamports && best_quote >= target_lamports =>
        {
            (best_amount, best_quote)
        }
        Some((_, best_quote)) if quoted >= target_lamports && best_quote < target_lamports => {
            (amount, quoted)
        }
        Some((best_amount, best_quote))
            if target_quote_distance(quoted, target_lamports)
                > target_quote_distance(best_quote, target_lamports) =>
        {
            (best_amount, best_quote)
        }
        Some((best_amount, best_quote))
            if target_quote_distance(quoted, target_lamports)
                == target_quote_distance(best_quote, target_lamports)
                && best_amount <= amount =>
        {
            (best_amount, best_quote)
        }
        _ => (amount, quoted),
    }
}

fn target_quote_distance(quoted: u64, target: u64) -> u64 {
    quoted.abs_diff(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_sizing_uses_quote_ratio() {
        let amount = choose_target_sized_token_amount(1_000, 250, |input| Ok(input)).unwrap();
        assert_eq!(amount, 250);
    }

    #[test]
    fn target_sizing_prefers_reaching_target_on_equal_distance() {
        let amount = choose_target_sized_token_amount(10, 15, |input| Ok(input * 2)).unwrap();
        assert_eq!(amount, 8);
    }

    #[test]
    fn target_sizing_prefers_feasible_quote_over_closer_shortfall() {
        let amount = choose_target_sized_token_amount(10, 15, |input| Ok(input * 2)).unwrap();
        assert_eq!(amount, 8);
    }

    #[test]
    fn target_sizing_uses_smallest_amount_on_exact_quote_plateau() {
        let amount = choose_target_sized_token_amount(100, 10, |input| Ok(input / 10)).unwrap();
        assert_eq!(amount, 100);

        let amount = choose_target_sized_token_amount(100, 5, |input| Ok(input / 10)).unwrap();
        assert_eq!(amount, 50);
    }

    #[test]
    fn target_sizing_rejects_unreachable_target() {
        let err = choose_target_sized_token_amount(10, 50, |input| Ok(input * 2))
            .expect_err("target above full balance quote should be rejected");
        assert!(err.contains("Not enough tokens to sell for 0.00000005 SOL"));
    }

    #[test]
    fn target_sizing_rejects_empty_balance() {
        assert!(choose_target_sized_token_amount(0, 1, |_| Ok(1)).is_err());
    }

    #[test]
    fn target_sizing_binary_searches_curved_quotes() {
        let amount =
            choose_target_sized_token_amount(1_000, 250_000, |input| Ok(input * input)).unwrap();
        assert_eq!(amount, 500);
    }

    #[test]
    fn target_estimate_uses_full_balance_quote_ratio() {
        assert_eq!(target_amount_estimate(1_000_000, 2_500, 10_000), 250_000);
        assert_eq!(target_amount_estimate(10, 1, 1_000), 1);
    }

    #[test]
    fn unreachable_target_message_formats_sol_amounts() {
        assert_eq!(
            unreachable_target_message(1_000_000_000, 750_000_000),
            "Not enough tokens to sell for 1 SOL. Current balance is quoted for up to 0.75 SOL after fees."
        );
    }
}
