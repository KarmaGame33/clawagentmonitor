//! Logique partagée pour ClawAgentMonitor.
//!
//! Modules à venir : cli (wrapper openclaw --json), models (structs serde),
//! agent_state, gateway, watchdog.

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn version_is_set() {
        assert!(!version().is_empty());
    }
}
