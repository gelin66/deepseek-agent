//! Product locale identity and deterministic locale resolution.
//!
//! This module is independent of any presentation surface. TUI translations
//! remain owned by `codewhale-tui`; headless application composition can use
//! the same locale value and precedence without depending on the TUI crate.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    Ja,
    ZhHans,
    ZhHant,
    PtBr,
    Es419,
    Vi,
    Ko,
}

/// Product default when neither configuration nor the environment resolves to
/// a supported locale.
pub const DEFAULT_LOCALE: Locale = Locale::ZhHans;

impl Locale {
    #[must_use]
    pub const fn tag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
            Self::ZhHans => "zh-Hans",
            Self::ZhHant => "zh-Hant",
            Self::PtBr => "pt-BR",
            Self::Es419 => "es-419",
            Self::Vi => "vi",
            Self::Ko => "ko",
        }
    }

    #[must_use]
    pub const fn translation_target_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ja => "Japanese (日本語)",
            Self::ZhHans => "Simplified Chinese (简体中文)",
            Self::ZhHant => "Traditional Chinese (繁體中文)",
            Self::PtBr => "Brazilian Portuguese (Português do Brasil)",
            Self::Es419 => "Latin American Spanish (Español latinoamericano)",
            Self::Vi => "Vietnamese (Tiếng Việt)",
            Self::Ko => "Korean (한국어)",
        }
    }

    /// Every locale exposed by product configuration and UI pickers.
    #[must_use]
    pub const fn shipped() -> &'static [Self] {
        &[
            Self::En,
            Self::Ja,
            Self::ZhHans,
            Self::ZhHant,
            Self::PtBr,
            Self::Es419,
            Self::Vi,
            Self::Ko,
        ]
    }

    /// Locales whose TUI message packs are held to English-key parity.
    ///
    /// This remains metadata only; translated strings stay in `codewhale-tui`.
    #[must_use]
    pub const fn shipped_complete() -> &'static [Self] {
        &[
            Self::En,
            Self::Ja,
            Self::ZhHans,
            Self::PtBr,
            Self::Es419,
            Self::Vi,
            Self::Ko,
        ]
    }

    #[must_use]
    pub const fn is_partial_pack(self) -> bool {
        matches!(self, Self::ZhHant)
    }
}

/// Normalize a configured locale value to the product's canonical tag.
///
/// Empty, `auto`, and `system` all preserve automatic environment resolution.
#[must_use]
pub fn normalize_configured_locale(input: &str) -> Option<&'static str> {
    let normalized = normalize_locale_input(input);
    if matches!(normalized.as_str(), "" | "auto" | "system") {
        return Some("auto");
    }
    parse_locale(&normalized).map(Locale::tag)
}

/// Resolve an explicit locale or the current process locale.
#[must_use]
pub fn resolve_locale(setting: &str) -> Locale {
    resolve_locale_with_env(setting, |key| std::env::var(key).ok())
}

/// Resolve a locale with an injected environment reader.
///
/// The injection keeps precedence deterministic in tests and is also useful to
/// callers that snapshot their environment before starting concurrent runs.
#[must_use]
pub fn resolve_locale_with_env<F>(setting: &str, env: F) -> Locale
where
    F: Fn(&str) -> Option<String>,
{
    let normalized = normalize_locale_input(setting);
    if !matches!(normalized.as_str(), "" | "auto" | "system") {
        return parse_locale(&normalized).unwrap_or(DEFAULT_LOCALE);
    }

    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(value) = env(key)
            && let Some(locale) = parse_locale(&normalize_locale_input(&value))
        {
            return locale;
        }
    }

    DEFAULT_LOCALE
}

fn normalize_locale_input(input: &str) -> String {
    input
        .split('.')
        .next()
        .unwrap_or(input)
        .split('@')
        .next()
        .unwrap_or(input)
        .trim()
        .replace('_', "-")
        .to_lowercase()
}

fn parse_locale(value: &str) -> Option<Locale> {
    if value == "c" || value == "posix" || value.starts_with("en") {
        return Some(Locale::En);
    }
    if value.starts_with("ja") {
        return Some(Locale::Ja);
    }
    if value.starts_with("zh") {
        if value.contains("hant")
            || value.contains("-tw")
            || value.contains("-hk")
            || value.contains("-mo")
        {
            return Some(Locale::ZhHant);
        }
        return Some(Locale::ZhHans);
    }
    if value.starts_with("pt") || value == "br" {
        return Some(Locale::PtBr);
    }
    if value.starts_with("es") {
        return Some(Locale::Es419);
    }
    if value.starts_with("vi") {
        return Some(Locale::Vi);
    }
    if value.starts_with("ko") {
        return Some(Locale::Ko);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_tags_normalize_without_changing_supported_semantics() {
        assert_eq!(normalize_configured_locale("auto"), Some("auto"));
        assert_eq!(normalize_configured_locale("system"), Some("auto"));
        assert_eq!(normalize_configured_locale("en_US.UTF-8"), Some("en"));
        assert_eq!(normalize_configured_locale("ja_JP.UTF-8"), Some("ja"));
        assert_eq!(normalize_configured_locale("zh-CN"), Some("zh-Hans"));
        assert_eq!(normalize_configured_locale("zh-TW"), Some("zh-Hant"));
        assert_eq!(normalize_configured_locale("zh_HK.UTF-8"), Some("zh-Hant"));
        assert_eq!(normalize_configured_locale("pt-PT"), Some("pt-BR"));
        assert_eq!(normalize_configured_locale("es-MX"), Some("es-419"));
        assert_eq!(normalize_configured_locale("vi-VN"), Some("vi"));
        assert_eq!(normalize_configured_locale("ko-KR"), Some("ko"));
        assert_eq!(normalize_configured_locale("ar"), None);
    }

    #[test]
    fn explicit_locale_wins_over_every_environment_value() {
        assert_eq!(
            resolve_locale_with_env("ja", |_| Some("pt_BR.UTF-8".to_owned())),
            Locale::Ja
        );
        assert_eq!(
            resolve_locale_with_env("en", |_| Some("zh_CN.UTF-8".to_owned())),
            Locale::En
        );
    }

    #[test]
    fn automatic_resolution_uses_lc_all_then_lc_messages_then_lang() {
        let resolved = resolve_locale_with_env("auto", |key| match key {
            "LC_ALL" => Some("ja_JP.UTF-8".to_owned()),
            "LC_MESSAGES" => Some("pt_BR.UTF-8".to_owned()),
            "LANG" => Some("zh_TW.UTF-8".to_owned()),
            _ => None,
        });
        assert_eq!(resolved, Locale::Ja);

        let resolved = resolve_locale_with_env("auto", |key| match key {
            "LC_MESSAGES" => Some("pt_BR.UTF-8".to_owned()),
            "LANG" => Some("zh_TW.UTF-8".to_owned()),
            _ => None,
        });
        assert_eq!(resolved, Locale::PtBr);

        let resolved = resolve_locale_with_env("auto", |key| {
            (key == "LANG").then(|| "zh_TW.UTF-8".to_owned())
        });
        assert_eq!(resolved, Locale::ZhHant);
    }

    #[test]
    fn unsupported_or_missing_locale_uses_product_default() {
        assert_eq!(resolve_locale_with_env("auto", |_| None), DEFAULT_LOCALE);
        assert_eq!(resolve_locale_with_env("ar", |_| None), DEFAULT_LOCALE);
        assert_eq!(
            resolve_locale_with_env("auto", |_| Some("ar_EG.UTF-8".to_owned())),
            DEFAULT_LOCALE
        );
    }
}
