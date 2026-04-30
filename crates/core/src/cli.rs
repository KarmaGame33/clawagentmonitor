//! Wrapper async autour du CLI `openclaw`.
//!
//! Toutes les fonctions spawnent `openclaw … --json` via `tokio::process::Command`,
//! capturent la sortie standard, et la parsent vers les structs de [`crate::models`].
//!
//! Un timeout dur est appliqué sur chaque appel via [`tokio::time::timeout`] pour
//! garantir que la GUI ne bloque jamais, même si le CLI met plus longtemps que prévu.

use std::ffi::OsStr;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use tokio::process::Command;
use tokio::time::timeout;

use crate::models::{GatewayProbe, StatusAll, TasksList};

/// Configuration partagée pour tous les appels au CLI.
#[derive(Debug, Clone)]
pub struct ClawCli {
    /// Chemin vers le binaire `openclaw` (défaut: PATH lookup).
    pub binary: String,
    /// Timeout dur pour chaque appel (défaut 8s).
    pub default_timeout: Duration,
}

impl Default for ClawCli {
    fn default() -> Self {
        Self {
            binary: "openclaw".to_string(),
            default_timeout: Duration::from_secs(15),
        }
    }
}

impl ClawCli {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_binary(mut self, bin: impl Into<String>) -> Self {
        self.binary = bin.into();
        self
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.default_timeout = t;
        self
    }

    /// Statut global. Marche aussi quand le gateway est éteint.
    /// Timeout volontairement plus long (45s) car cette commande agrège
    /// channels + sessions + memory + audit ; quand le gateway est UP elle
    /// peut prendre 30s+ sur une grosse install.
    pub async fn status_all(&self) -> Result<StatusAll> {
        self.run_json(
            &["status", "--all", "--json"],
            Duration::from_secs(45),
        )
        .await
    }

    /// Liste des tâches. `runtime` peut être `Some("subagent")`, `Some("cli")`, etc.
    pub async fn tasks_list(
        &self,
        runtime: Option<&str>,
        status: Option<&str>,
    ) -> Result<TasksList> {
        let mut args: Vec<&str> = vec!["tasks", "list", "--json"];
        if let Some(r) = runtime {
            args.push("--runtime");
            args.push(r);
        }
        if let Some(s) = status {
            args.push("--status");
            args.push(s);
        }
        self.run_json(&args, self.default_timeout).await
    }

    /// Probe du gateway. Timeout court.
    pub async fn gateway_probe(&self) -> Result<GatewayProbe> {
        self.run_json(
            &["gateway", "probe", "--json", "--timeout", "2500"],
            Duration::from_secs(6),
        )
        .await
    }

    /// Restart simple (palier 1 de l'escalade non-destructive).
    pub async fn gateway_restart(&self) -> Result<()> {
        self.run_silent(&["gateway", "restart"], Duration::from_secs(20))
            .await
    }

    /// Doctor --fix (palier 2).
    pub async fn doctor_fix(&self) -> Result<()> {
        self.run_silent(&["doctor", "--fix"], Duration::from_secs(60))
            .await
    }

    /// Réinstallation forcée du service (palier 3, dernier recours).
    pub async fn gateway_install_force(&self) -> Result<()> {
        self.run_silent(
            &["gateway", "install", "--force"],
            Duration::from_secs(60),
        )
        .await
    }

    async fn run_json<S, T>(&self, args: &[S], deadline: Duration) -> Result<T>
    where
        S: AsRef<OsStr>,
        T: DeserializeOwned,
    {
        let output = self.run_capture(args, deadline).await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        serde_json::from_str::<T>(&stdout).with_context(|| {
            format!(
                "failed to parse openclaw JSON output (first 200 bytes: {:.200})",
                stdout
            )
        })
    }

    async fn run_silent<S>(&self, args: &[S], deadline: Duration) -> Result<()>
    where
        S: AsRef<OsStr>,
    {
        let output = self.run_capture(args, deadline).await?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(anyhow!(
                "openclaw exited with status {}: {}",
                output.status,
                stderr.trim()
            ))
        }
    }

    async fn run_capture<S>(
        &self,
        args: &[S],
        deadline: Duration,
    ) -> Result<std::process::Output>
    where
        S: AsRef<OsStr>,
    {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args).kill_on_drop(true);
        let fut = cmd.output();
        let out = timeout(deadline, fut)
            .await
            .map_err(|_| anyhow!("openclaw call timed out after {:?}", deadline))?
            .with_context(|| format!("failed to spawn `{}`", self.binary))?;
        Ok(out)
    }
}
