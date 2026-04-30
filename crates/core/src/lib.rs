//! Logique partagée pour ClawAgentMonitor.
//!
//! Cette crate est volontairement agnostique de l'UI : elle ne connaît
//! ni Slint ni la fenêtre. Elle fournit :
//!
//! - [`models`] : structs serde des sorties JSON du CLI `openclaw`.
//! - [`cli`] : wrapper async qui exécute `openclaw … --json` et parse la sortie.
//! - [`agent_state`] : heuristique vert/orange/rouge/gris.
//! - [`gateway`] : probe + restart (escalade non-destructive).
//! - [`watchdog`] : boucle tokio in-process avec anti crash-loop.
//! - [`config`] : préférences utilisateur persistées (auto-restart, auto-launch, …).
//! - [`autostart`] : wrapper cross-platform pour le démarrage au login.

pub mod agent_state;
pub mod autostart;
pub mod cli;
pub mod config;
pub mod gateway;
pub mod models;
pub mod watchdog;

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
