//! Binaire ClawAgentMonitor : pont entre `claw-core` (logique async) et la
//! GUI Slint (event loop natif).
//!
//! Le pattern : un Runtime tokio multi-thread tourne en parallèle de la GUI.
//! Une tâche périodique calcule un `StatusSnapshot` puis le renvoie au thread
//! Slint via `slint::invoke_from_event_loop`. Le bouton "Relancer Gateway"
//! déclenche `gateway::restart_with_escalation` en background, avec un état
//! "restart-in-progress" qui désactive le bouton pendant l'opération.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use claw_core::agent_state::{build_snapshot, AgentStatus, StatusSnapshot};
use claw_core::cli::ClawCli;
use claw_core::gateway::{restart_with_escalation, RestartMode};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Mutex;

slint::include_modules!();

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(
        "ClawAgentMonitor v{} starting (claw-core v{})",
        env!("CARGO_PKG_VERSION"),
        claw_core::version()
    );

    // Runtime tokio multi-thread (séparé du thread GUI Slint)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    let app = MainWindow::new()?;

    // État partagé : verrouille les actions concurrentes (clic restart x N)
    let cli = Arc::new(ClawCli::new());
    let restart_lock = Arc::new(Mutex::new(()));

    // Branche le bouton "Relancer Gateway"
    {
        let app_weak = app.as_weak();
        let cli = cli.clone();
        let restart_lock = restart_lock.clone();
        let rt_handle = rt.handle().clone();
        app.on_restart_gateway(move || {
            let app_weak = app_weak.clone();
            let cli = cli.clone();
            let restart_lock = restart_lock.clone();
            rt_handle.spawn(async move {
                let _guard = match restart_lock.try_lock() {
                    Ok(g) => g,
                    Err(_) => {
                        tracing::warn!("restart already in progress, ignoring click");
                        return;
                    }
                };
                set_restart_in_progress(&app_weak, true);
                tracing::info!("restart-gateway: starting Safe escalation");
                match restart_with_escalation(&cli, RestartMode::Safe).await {
                    Ok(report) => {
                        tracing::info!(
                            steps = report.steps.len(),
                            final_ok = report.final_ok,
                            "restart escalation finished"
                        );
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "restart escalation errored");
                    }
                }
                set_restart_in_progress(&app_weak, false);
            });
        });
    }

    // Boucle de polling toutes les 5s
    {
        let app_weak = app.as_weak();
        let cli = cli.clone();
        rt.spawn(async move {
            // Premier tick immédiat pour ne pas attendre 5s avant d'afficher quelque chose
            poll_and_push(&cli, &app_weak).await;
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                poll_and_push(&cli, &app_weak).await;
            }
        });
    }

    app.run()?;
    Ok(())
}

/// Effectue un cycle de polling : status_all + tasks_list (running) en parallèle,
/// calcule le snapshot, puis pousse vers la GUI sur l'event-loop Slint.
async fn poll_and_push(cli: &ClawCli, app_weak: &slint::Weak<MainWindow>) {
    let started = std::time::Instant::now();
    tracing::debug!("polling cycle: starting");
    let (status_res, tasks_res) = tokio::join!(
        cli.status_all(),
        cli.tasks_list(None, Some("running")),
    );

    let snapshot: Result<StatusSnapshot, anyhow::Error> = match (status_res, tasks_res) {
        (Ok(s), Ok(t)) => Ok(build_snapshot(&s, Some(&t))),
        (Ok(s), Err(e)) => {
            tracing::warn!(error = %e, "tasks_list failed, snapshot will be incomplete");
            Ok(build_snapshot(&s, None))
        }
        (Err(e), _) => {
            tracing::warn!(error = %e, "status_all failed, skipping update");
            Err(e)
        }
    };

    if let Ok(snap) = snapshot {
        tracing::info!(
            agents = snap.agents.len(),
            running = snap.total_running_tasks,
            gateway = ?snap.gateway,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "snapshot pushed to UI"
        );
        push_snapshot_to_ui(app_weak, snap);
    }
}

/// Pousse le snapshot calculé sur l'event-loop Slint pour mettre à jour les Properties.
fn push_snapshot_to_ui(app_weak: &slint::Weak<MainWindow>, snap: StatusSnapshot) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(app) = app_weak.upgrade() else {
            return;
        };
        let agents_ui: Vec<AgentInfo> = snap
            .agents
            .iter()
            .map(agent_to_ui)
            .collect();
        app.set_agents(ModelRc::new(VecModel::from(agents_ui)));
        app.set_gateway_status(SharedString::from(snap.gateway.as_str()));
        app.set_gateway_runtime(SharedString::from(
            snap.gateway_runtime_short.clone().unwrap_or_default(),
        ));
        app.set_total_running_tasks(snap.total_running_tasks as i32);
        app.set_total_failures(snap.total_failures as i32);
        app.set_last_update(SharedString::from(now_hms()));
    });
}

/// Convertit un AgentStatus claw-core en AgentInfo Slint.
fn agent_to_ui(a: &AgentStatus) -> AgentInfo {
    AgentInfo {
        id: SharedString::from(a.id.clone()),
        name: SharedString::from(a.name.clone()),
        model: SharedString::from(a.model.clone().unwrap_or_else(|| "?".into())),
        status: SharedString::from(a.status.as_str()),
        note: SharedString::from(a.note.clone()),
        last_active: SharedString::from(format_age(a.last_active_age_ms)),
        sessions_count: a.sessions_count as i32,
        running_tasks: a.running_tasks as i32,
    }
}

fn set_restart_in_progress(app_weak: &slint::Weak<MainWindow>, in_progress: bool) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_restart_in_progress(in_progress);
        }
    });
}

fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn format_age(ms: Option<i64>) -> String {
    let Some(ms) = ms else { return "-".into() };
    let secs = ms / 1000;
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}min");
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{hours}h");
    }
    let days = hours / 24;
    format!("{days}j")
}
