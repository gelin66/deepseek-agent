pub fn tenant_policy(tenant: &str) -> &'static str {
    if tenant.is_empty() {
        "deny"
    } else {
        "inherit"
    }
}
