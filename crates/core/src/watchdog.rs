//! Watchdog in-process pour le Gateway OpenClaw.
//!
//! Tourne comme une [`tokio::task`] dans le process GUI (option B retenue).
//! Cycle : ping périodique du gateway → si KO, escalade non-destructive
//! ([`crate::gateway::restart_with_escalation`]) → anti crash-loop (≥5 fails
//! en 5 min déclenche une pause de 10 min).
//!
//! Activable / désactivable depuis la GUI via [`WatchdogHandle::cancel`].

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::cli::ClawCli;
use crate::gateway::{restart_with_escalation, RestartMode, RestartReport};

#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// Intervalle entre deux probes (défaut 5 min).
    pub interval: Duration,
    /// Mode d'agressivité.
    pub mode: RestartMode,
    /// Fenêtre pour la détection de crash-loop (défaut 5 min).
    pub crash_window: Duration,
    /// Nombre d'échecs dans la fenêtre avant pause (défaut 5).
    pub crash_threshold: usize,
    /// Pause après détection de crash-loop (défaut 10 min).
    pub crash_pause: Duration,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(300),
            mode: RestartMode::Safe,
            crash_window: Duration::from_secs(300),
            crash_threshold: 5,
            crash_pause: Duration::from_secs(600),
        }
    }
}

/// Évènement publié par le watchdog (consommable par la GUI pour afficher le journal).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchdogEvent {
    Started,
    Probe {
        ok: bool,
        ts_ms: i64,
    },
    RestartAttempted {
        report_summary: String,
        final_ok: bool,
    },
    CrashLoopPause {
        until_ms: i64,
    },
    Stopped,
    Error {
        message: String,
    },
}

pub struct WatchdogHandle {
    handle: JoinHandle<()>,
}

impl WatchdogHandle {
    pub fn cancel(self) {
        self.handle.abort();
    }
}

/// Démarre le watchdog. Retourne un handle (pour stopper) et un Receiver d'évènements.
pub fn spawn(
    cli: ClawCli,
    config: WatchdogConfig,
) -> (WatchdogHandle, mpsc::Receiver<WatchdogEvent>) {
    let (tx, rx) = mpsc::channel::<WatchdogEvent>(32);
    let handle = tokio::spawn(async move { run_loop(cli, config, tx).await });
    (WatchdogHandle { handle }, rx)
}

async fn run_loop(cli: ClawCli, config: WatchdogConfig, tx: mpsc::Sender<WatchdogEvent>) {
    let _ = tx.send(WatchdogEvent::Started).await;
    info!(?config, "watchdog started");
    let mut crash_window: VecDeque<Instant> = VecDeque::new();

    loop {
        let probe_ok = crate::gateway::is_reachable(&cli).await;
        let _ = tx
            .send(WatchdogEvent::Probe {
                ok: probe_ok,
                ts_ms: now_ms(),
            })
            .await;

        if !probe_ok {
            // Anti crash-loop
            let now = Instant::now();
            crash_window.push_back(now);
            while crash_window
                .front()
                .map(|t| now.duration_since(*t) > config.crash_window)
                .unwrap_or(false)
            {
                crash_window.pop_front();
            }

            if crash_window.len() >= config.crash_threshold {
                let until = now + config.crash_pause;
                warn!(
                    fails = crash_window.len(),
                    pause = ?config.crash_pause,
                    "crash-loop detected, pausing watchdog"
                );
                let _ = tx
                    .send(WatchdogEvent::CrashLoopPause {
                        until_ms: now_ms() + config.crash_pause.as_millis() as i64,
                    })
                    .await;
                sleep(config.crash_pause).await;
                crash_window.clear();
                let _ = tokio::time::timeout(Duration::from_secs(0), async {}).await;
                let _ = until;
                continue;
            }

            match restart_with_escalation(&cli, config.mode).await {
                Ok(report) => {
                    let summary = summarise(&report);
                    info!(?report.final_ok, "restart escalation finished");
                    let _ = tx
                        .send(WatchdogEvent::RestartAttempted {
                            report_summary: summary,
                            final_ok: report.final_ok,
                        })
                        .await;
                    if report.final_ok {
                        crash_window.clear();
                    }
                }
                Err(e) => {
                    warn!(error = %e, "restart escalation errored");
                    let _ = tx
                        .send(WatchdogEvent::Error {
                            message: e.to_string(),
                        })
                        .await;
                }
            }
        }

        sleep(config.interval).await;
    }
}

fn summarise(r: &RestartReport) -> String {
    let parts: Vec<String> = r
        .steps
        .iter()
        .map(|s| {
            format!(
                "{}={}",
                s.action,
                if s.probe_after_ok { "ok" } else { "ko" }
            )
        })
        .collect();
    parts.join(" → ")
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Dossier d'état persistant cross-platform pour le watchdog
/// (lock, journal, history). Utilise `dirs::state_dir` sous Linux,
/// `dirs::data_local_dir` sous macOS/Windows.
pub fn state_dir() -> Option<PathBuf> {
    let base = dirs::state_dir().or_else(dirs::data_local_dir)?;
    Some(base.join("clawagentmonitor"))
}

/// Crée le dossier d'état s'il n'existe pas.
pub fn ensure_state_dir() -> Result<PathBuf> {
    let p = state_dir().ok_or_else(|| anyhow::anyhow!("could not resolve state dir"))?;
    std::fs::create_dir_all(&p)?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_reasonable() {
        let c = WatchdogConfig::default();
        assert_eq!(c.crash_threshold, 5);
        assert!(c.crash_pause > c.crash_window || c.crash_pause >= Duration::from_secs(300));
    }

    #[test]
    fn state_dir_resolves() {
        let p = state_dir();
        assert!(p.is_some());
        assert!(p.unwrap().to_string_lossy().contains("clawagentmonitor"));
    }
}
