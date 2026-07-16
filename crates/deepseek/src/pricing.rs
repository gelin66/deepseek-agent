//! Official DeepSeek token accounting and first-party price projection.
//!
//! Price rows live beside the transport accounting that consumes them. This
//! deliberately recognizes only model ids accepted by the official DeepSeek
//! route; foreign hosts and provider catalogs must not inherit these rates.

use codewhale_runtime::Usage;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CostEstimate {
    pub usd: f64,
    pub cny: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurrencyPricing {
    pub cache_hit_per_million: f64,
    pub cache_miss_per_million: f64,
    pub output_per_million: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricing {
    pub usd: CurrencyPricing,
    pub cny: CurrencyPricing,
}

pub(crate) fn response_cost(model: &str, usage: Option<&Usage>) -> Option<(f64, f64)> {
    calculate_turn_cost_estimate(model, usage?).map(|cost| (cost.usd, cost.cny))
}

#[must_use]
pub fn calculate_turn_cost_estimate(model: &str, usage: &Usage) -> Option<CostEstimate> {
    let pricing = pricing_for_official_model(model)?;
    Some(CostEstimate {
        usd: cost_with_pricing(pricing.usd, usage),
        cny: cost_with_pricing(pricing.cny, usage),
    })
}

#[must_use]
pub fn pricing_for_official_model(model: &str) -> Option<ModelPricing> {
    match model.trim() {
        "deepseek-v4-pro" => Some(ModelPricing {
            usd: CurrencyPricing {
                cache_hit_per_million: 0.003625,
                cache_miss_per_million: 0.435,
                output_per_million: 0.87,
            },
            cny: CurrencyPricing {
                cache_hit_per_million: 0.025,
                cache_miss_per_million: 3.0,
                output_per_million: 6.0,
            },
        }),
        "deepseek-v4-flash" => Some(ModelPricing {
            usd: CurrencyPricing {
                cache_hit_per_million: 0.0028,
                cache_miss_per_million: 0.14,
                output_per_million: 0.28,
            },
            cny: CurrencyPricing {
                cache_hit_per_million: 0.02,
                cache_miss_per_million: 1.0,
                output_per_million: 2.0,
            },
        }),
        _ => None,
    }
}

fn cost_with_pricing(pricing: CurrencyPricing, usage: &Usage) -> f64 {
    let categorized = usage
        .cache_hit_tokens
        .saturating_add(usage.cache_miss_tokens)
        .saturating_add(usage.cache_write_tokens);
    let uncategorized = usage.input_tokens.saturating_sub(categorized);
    let cache_miss = usage
        .cache_miss_tokens
        .saturating_add(usage.cache_write_tokens)
        .saturating_add(uncategorized);
    (usage.cache_hit_tokens as f64 / 1_000_000.0) * pricing.cache_hit_per_million
        + (cache_miss as f64 / 1_000_000.0) * pricing.cache_miss_per_million
        + (usage.output_tokens as f64 / 1_000_000.0) * pricing.output_per_million
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_cache_classes_and_does_not_double_bill_reasoning() {
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_hit_tokens: 250_000,
            cache_miss_tokens: 700_000,
            cache_write_tokens: 50_000,
            reasoning_tokens: 80_000,
            ..Usage::default()
        };
        let estimate = calculate_turn_cost_estimate("deepseek-v4-pro", &usage).unwrap();
        let expected_usd = 0.25 * 0.003625 + 0.75 * 0.435 + 0.1 * 0.87;
        let expected_cny = 0.25 * 0.025 + 0.75 * 3.0 + 0.1 * 6.0;
        assert!((estimate.usd - expected_usd).abs() < 1e-12);
        assert!((estimate.cny - expected_cny).abs() < 1e-12);
    }

    #[test]
    fn legacy_aliases_and_foreign_slugs_are_not_first_party_priced() {
        for model in [
            "deepseek-chat",
            "deepseek-reasoner",
            "deepseek-v4pro",
            "deepseek/deepseek-v4-pro",
            "deepseek-ai/deepseek-v4-pro",
        ] {
            assert!(calculate_turn_cost_estimate(model, &Usage::default()).is_none());
        }
    }
}
