//! Configuration utilisateur persistée pour ClawAgentMonitor.
//!
//! Stockée en JSON dans `state_dir()/config.json` (résolu via `dirs::state_dir`
//! sous Linux, `dirs::data_local_dir` sous macOS/Windows).
//!
//! Ne contient que les préférences UI : reachability, etc. ne sont pas persistées.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::watchdog::state_dir;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Active la surveillance automatique du gateway par le watchdog in-process.
    pub auto_restart_enabled: bool,
    /// Autorise le palier 3 (`gateway install --force`) sans confirmation.
    /// Subordonné à `auto_restart_enabled` côté UI.
    pub auto_aggressive: bool,
    /// Lance ClawAgentMonitor au login (via `auto-launch`).
    pub start_at_login: bool,
    /// Intervalle entre deux probes du watchdog (en secondes).
    pub watchdog_interval_secs: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            auto_restart_enabled: false,
            auto_aggressive: false,
            start_at_login: false,
            watchdog_interval_secs: 300,
        }
    }
}

impl AppConfig {
    /// Chemin canonique du fichier de config.
    pub fn config_path() -> Option<PathBuf> {
        state_dir().map(|d| d.join("config.json"))
    }

    /// Charge depuis le disque, ou retourne les valeurs par défaut si absent / corrompu.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<AppConfig>(&s).unwrap_or_else(|e| {
                tracing::warn!(path = %path.display(), error = %e, "config corrupted, using defaults");
                Self::default()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "failed to read config");
                Self::default()
            }
        }
    }

    /// Sauvegarde sur disque (crée le dossier si besoin).
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()
            .context("could not resolve config path (state_dir unavailable)")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating config dir {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(self).context("serializing AppConfig")?;
        std::fs::write(&path, json)
            .with_context(|| format!("writing config to {}", path.display()))?;
        tracing::debug!(path = %path.display(), "config saved");
        Ok(())
    }

    /// Sauvegarde sur disque, mais sans propager les erreurs (utile dans les callbacks UI).
    pub fn save_best_effort(&self) {
        if let Err(e) = self.save() {
            tracing::warn!(error = %e, "failed to save config");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_reasonable() {
        let c = AppConfig::default();
        assert!(!c.auto_restart_enabled);
        assert!(!c.auto_aggressive);
        assert!(!c.start_at_login);
        assert!(c.watchdog_interval_secs >= 60);
    }

    #[test]
    fn roundtrip_serialization() {
        let c = AppConfig {
            auto_restart_enabled: true,
            auto_aggressive: true,
            start_at_login: false,
            watchdog_interval_secs: 120,
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(back.auto_restart_enabled);
        assert!(back.auto_aggressive);
        assert!(!back.start_at_login);
        assert_eq!(back.watchdog_interval_secs, 120);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let json = "{}";
        let c: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!c.auto_restart_enabled);
        assert_eq!(c.watchdog_interval_secs, 300);
    }

    #[test]
    fn partial_json_uses_defaults_for_missing_fields() {
        let json = r#"{"auto_restart_enabled": true}"#;
        let c: AppConfig = serde_json::from_str(json).unwrap();
        assert!(c.auto_restart_enabled);
        assert!(!c.auto_aggressive);
        assert_eq!(c.watchdog_interval_secs, 300);
    }
}
