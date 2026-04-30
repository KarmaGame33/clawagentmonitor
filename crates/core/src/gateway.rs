//! Probe et redémarrage du Gateway OpenClaw.
//!
//! Le redémarrage suit une **escalade non-destructive** :
//! 1. `openclaw gateway restart`     (rapide, zéro effet de bord)
//! 2. `openclaw doctor --fix`        (correctifs courants)
//! 3. `openclaw gateway install --force` (réécrit le fichier `.service`,
//!    consentement utilisateur requis sauf en mode "auto-agressive").
//!
//! Le palier 3 ne touche jamais `~/.openclaw/openclaw.json` ni les workspaces.

use std::time::Duration;

use anyhow::Result;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::cli::ClawCli;

/// Niveau d'agressivité du redémarrage automatique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartMode {
    /// Manuel : un palier à la fois, avec consentement entre chaque.
    Manual,
    /// Auto : enchaîne automatiquement les paliers 1 et 2, **arrête avant 3**.
    Safe,
    /// Auto-agressive : enchaîne automatiquement les paliers 1, 2 et 3.
    Aggressive,
}

/// Résultat d'une tentative de remise en route.
#[derive(Debug, Clone)]
pub struct RestartReport {
    pub steps: Vec<RestartStep>,
    pub final_ok: bool,
}

#[derive(Debug, Clone)]
pub struct RestartStep {
    pub action: &'static str,
    pub success: bool,
    pub probe_after_ok: bool,
    pub error: Option<String>,
}

/// Vérifie la reachability du gateway via `openclaw gateway probe --json`.
pub async fn is_reachable(cli: &ClawCli) -> bool {
    match cli.gateway_probe().await {
        Ok(p) => p.ok,
        Err(e) => {
            warn!(error = %e, "gateway probe failed");
            false
        }
    }
}

/// Vérifie quelques fois (avec backoff) que le gateway est revenu après une action.
async fn probe_with_retries(cli: &ClawCli, retries: usize, every: Duration) -> bool {
    for i in 0..retries {
        sleep(every).await;
        if is_reachable(cli).await {
            info!(attempt = i + 1, "gateway probe OK after restart action");
            return true;
        }
    }
    false
}

/// Lance l'escalade non-destructive selon le mode.
pub async fn restart_with_escalation(cli: &ClawCli, mode: RestartMode) -> Result<RestartReport> {
    let mut steps = Vec::new();

    // Palier 1 : gateway restart
    let s = run_step(cli, "gateway restart", cli.gateway_restart()).await;
    let ok1 = s.success && s.probe_after_ok;
    steps.push(s);
    if ok1 {
        return Ok(RestartReport {
            steps,
            final_ok: true,
        });
    }

    if mode == RestartMode::Manual {
        return Ok(RestartReport {
            steps,
            final_ok: false,
        });
    }

    // Palier 2 : doctor --fix
    let s = run_step(cli, "doctor --fix", cli.doctor_fix()).await;
    let ok2 = s.success && s.probe_after_ok;
    steps.push(s);
    if ok2 {
        return Ok(RestartReport {
            steps,
            final_ok: true,
        });
    }

    if mode != RestartMode::Aggressive {
        warn!("escalation stopped at step 2 (consent required for step 3)");
        return Ok(RestartReport {
            steps,
            final_ok: false,
        });
    }

    // Palier 3 : gateway install --force
    warn!("escalation reached step 3: gateway install --force (auto-aggressive)");
    let s = run_step(cli, "gateway install --force", cli.gateway_install_force()).await;
    let ok3 = s.success && s.probe_after_ok;
    steps.push(s);

    Ok(RestartReport {
        steps,
        final_ok: ok3,
    })
}

async fn run_step(
    cli: &ClawCli,
    action: &'static str,
    fut: impl std::future::Future<Output = Result<()>>,
) -> RestartStep {
    info!(action, "running restart step");
    let mut step = RestartStep {
        action,
        success: false,
        probe_after_ok: false,
        error: None,
    };
    match fut.await {
        Ok(()) => {
            step.success = true;
            step.probe_after_ok = probe_with_retries(cli, 3, Duration::from_secs(5)).await;
            info!(action, probe_after_ok = step.probe_after_ok, "step done");
        }
        Err(e) => {
            step.error = Some(e.to_string());
            warn!(action, error = %e, "step failed");
        }
    }
    step
}
