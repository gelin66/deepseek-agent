use crate::resolver::canonical_service_name;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Service {
    pub name: String,
    pub priority: u16,
    pub healthy: bool,
}

#[derive(Clone, Debug)]
pub struct Registry {
    services: Vec<Service>,
}

impl Registry {
    pub fn new(services: Vec<Service>) -> Result<Self, &'static str> {
        Ok(Self { services })
    }

    pub fn resolve(&self, requested: &str) -> Result<Option<Service>, &'static str> {
        let requested = canonical_service_name(requested)?;
        Ok(self
            .services
            .iter()
            .filter(|service| service.healthy)
            .filter(|service| {
                canonical_service_name(&service.name)
                    .is_ok_and(|name| name == requested)
            })
            .min_by_key(|service| service.priority)
            .cloned())
    }
}
