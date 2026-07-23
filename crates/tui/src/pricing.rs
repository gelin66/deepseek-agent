//! Formatting for costs already accounted by the canonical runtime.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostCurrency {
    Usd,
    Cny,
}

impl CostCurrency {
    pub fn from_setting(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "usd" | "dollar" | "dollars" | "$" => Some(Self::Usd),
            "cny" | "rmb" | "yuan" | "¥" => Some(Self::Cny),
            _ => None,
        }
    }

    const fn symbol(self) -> &'static str {
        match self {
            Self::Usd => "$",
            Self::Cny => "¥",
        }
    }
}

#[must_use]
pub fn format_cost_amount(amount: f64, currency: CostCurrency) -> String {
    if !amount.is_finite() || amount <= 0.0 {
        return String::new();
    }
    if amount < 0.01 {
        format!("{}{amount:.4}", currency.symbol())
    } else {
        format!("{}{amount:.2}", currency.symbol())
    }
}

#[must_use]
pub fn format_cost_amount_precise(amount: f64, currency: CostCurrency) -> String {
    if !amount.is_finite() {
        return format!("{}—", currency.symbol());
    }
    format!("{}{amount:.6}", currency.symbol())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_projected_amounts() {
        assert_eq!(format_cost_amount(1.25, CostCurrency::Usd), "$1.25");
        assert_eq!(format_cost_amount(0.004, CostCurrency::Cny), "¥0.0040");
        assert_eq!(format_cost_amount(0.0, CostCurrency::Usd), "");
    }
}
