//! Bundled catalog lookup retained by pricing and legacy route metadata.
//!
//! The TUI has no provider picker, catalog dashboard, or live catalog producer.
//! Pricing therefore reads the bundled offline snapshot directly.

use codewhale_config::catalog::{CatalogOffering, CatalogSnapshot, bundled_catalog_offerings};

use crate::config::ApiProvider;

static BUNDLED_SNAPSHOT: std::sync::OnceLock<CatalogSnapshot> = std::sync::OnceLock::new();

fn bundled_snapshot() -> &'static CatalogSnapshot {
    BUNDLED_SNAPSHOT.get_or_init(|| CatalogSnapshot {
        offerings: bundled_catalog_offerings(),
    })
}

/// Maps an [`ApiProvider`] to its bundled-catalog provider id.
fn catalog_provider_id(provider: ApiProvider) -> &'static str {
    match provider {
        ApiProvider::DeepseekCN | ApiProvider::DeepseekAnthropic => "deepseek",
        ApiProvider::SiliconflowCn => "siliconflow",
        _ => provider.as_str(),
    }
}

/// Look up a bundled-catalog offering for `(provider, wire_model_id)`.
///
/// `None` means the bundled catalog has no price or route metadata for the id.
#[must_use]
pub fn catalog_offering_for_model(
    provider: ApiProvider,
    wire_model_id: &str,
) -> Option<CatalogOffering> {
    if provider == ApiProvider::OpenaiCodex {
        return None;
    }
    let catalog_id = catalog_provider_id(provider);
    let needle = wire_model_id.trim();
    if needle.is_empty() {
        return None;
    }
    bundled_snapshot()
        .offerings_for_provider(catalog_id)
        .into_iter()
        .find(|row| row.wire_model_id.eq_ignore_ascii_case(needle))
        .cloned()
}
