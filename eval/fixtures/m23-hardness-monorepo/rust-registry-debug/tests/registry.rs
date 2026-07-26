use m19_registry_debug::{Registry, Service, canonical_service_name};

fn service(name: &str, priority: u16, healthy: bool) -> Service {
    Service {
        name: name.to_owned(),
        priority,
        healthy,
    }
}

#[test]
fn canonicalizes_strict_service_names() {
    assert_eq!(canonical_service_name(" API-Core ").unwrap(), "api-core");
    for invalid in ["", "-api", "api-", "api--core", "api_core", "é"] {
        assert!(canonical_service_name(invalid).is_err(), "{invalid}");
    }
    assert!(canonical_service_name(&"a".repeat(41)).is_err());
}

#[test]
fn rejects_duplicate_canonical_names() {
    assert!(
        Registry::new(vec![
            service("API-Core", 10, true),
            service(" api-core ", 20, true),
        ])
        .is_err()
    );
}

#[test]
fn resolves_highest_priority_healthy_service() {
    let registry = Registry::new(vec![
        service("api-core", 10, true),
        service("API-Core", 50, false),
        service("api-core", 40, true),
    ]);
    assert!(registry.is_err(), "duplicates must be rejected before resolve");

    let registry = Registry::new(vec![
        service("api-core", 10, true),
        service("worker", 5, true),
    ])
    .unwrap();
    assert_eq!(registry.resolve(" API-Core ").unwrap().unwrap().priority, 10);
    assert_eq!(registry.resolve("missing").unwrap(), None);
}
