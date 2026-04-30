//! Binaire ClawAgentMonitor : pont entre `claw-core` (logique async) et la
//! GUI Slint (event loop natif).
//!
//! Pattern : un Runtime tokio multi-thread tourne en parallèle de la GUI.
//! Une tâche périodique calcule un `StatusSnapshot` et le pousse dans la GUI
//! via `slint::invoke_from_event_loop`. Les checkboxes Auto-restart /
//! Auto-agressive / Démarrer au login persistent dans `state_dir/config.json`
//! et démarrent / arrêtent le watchdog `claw-core::watchdog`.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use claw_core::agent_state::{build_snapshot, AgentStatus, StatusSnapshot};
use claw_core::cli::ClawCli;
use claw_core::config::AppConfig;
use claw_core::gateway::{restart_with_escalation, RestartMode};
use claw_core::watchdog::{spawn as spawn_watchdog, WatchdogConfig, WatchdogEvent, WatchdogHandle};
use claw_core::{autostart, watchdog};
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};
use tokio::sync::Mutex;

mod notify;
mod tray;

#[cfg(target_os = "linux")]

slint::include_modules!();

const LOG_MAX_ENTRIES: usize = 50;

/// État partagé entre les callbacks UI : permet de gérer le cycle de vie du
/// watchdog (start/stop/restart) sans corrompre la cohérence entre les threads.
struct AppState {
    cli: Arc<ClawCli>,
    config: Mutex<AppConfig>,
    watchdog_handle: Mutex<Option<WatchdogHandle>>,
    log: Mutex<VecDeque<LogEntry>>,
    #[cfg(target_os = "linux")]
    tray_handle: Mutex<Option<ksni::Handle<tray::ClawTray>>>,
}

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

    // S'assure que le state_dir existe (sert pour la config et plus tard les locks)
    if let Err(e) = watchdog::ensure_state_dir() {
        tracing::warn!(error = %e, "could not create state dir");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    let app = MainWindow::new()?;

    // Charge la config persistée
    let initial_config = AppConfig::load();
    tracing::info!(?initial_config, "config loaded");

    // Synchronise l'état persisté avec l'OS pour `start_at_login`
    let mut initial_config = initial_config;
    match autostart::is_enabled() {
        Ok(actually_enabled) if actually_enabled != initial_config.start_at_login => {
            tracing::info!(
                expected = initial_config.start_at_login,
                actually = actually_enabled,
                "autostart state diverges from config, OS state wins"
            );
            initial_config.start_at_login = actually_enabled;
            initial_config.save_best_effort();
        }
        Err(e) => tracing::debug!(error = %e, "autostart::is_enabled probe failed"),
        _ => {}
    }

    let state = Arc::new(AppState {
        cli: Arc::new(ClawCli::new()),
        config: Mutex::new(initial_config.clone()),
        watchdog_handle: Mutex::new(None),
        log: Mutex::new(VecDeque::with_capacity(LOG_MAX_ENTRIES)),
        #[cfg(target_os = "linux")]
        tray_handle: Mutex::new(None),
    });

    // Initialise les properties Slint depuis la config
    app.set_auto_restart_enabled(initial_config.auto_restart_enabled);
    app.set_auto_aggressive_enabled(initial_config.auto_aggressive);
    app.set_start_at_login_enabled(initial_config.start_at_login);
    app.set_notifications_enabled(initial_config.notifications_enabled);

    // Démarre le watchdog tout de suite si la config le demande
    if initial_config.auto_restart_enabled {
        spawn_or_replace_watchdog(rt.handle(), &state, app.as_weak());
    }

    // ===== Callbacks UI =====
    bind_restart_button(&app, &state, rt.handle().clone());
    bind_auto_restart_toggle(&app, &state, rt.handle().clone());
    bind_auto_aggressive_toggle(&app, &state, rt.handle().clone());
    bind_start_at_login_toggle(&app, &state);
    bind_notifications_toggle(&app, &state, rt.handle().clone());

    // ===== Polling périodique =====
    {
        let app_weak = app.as_weak();
        let cli = state.cli.clone();
        let rt_handle = rt.handle().clone();
        let state_for_poll = state.clone();
        rt.spawn(async move {
            poll_and_push(&cli, &app_weak, &rt_handle, &state_for_poll).await;
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                poll_and_push(&cli, &app_weak, &rt_handle, &state_for_poll).await;
            }
        });
    }

    // ===== Tray icon (Linux only, best-effort) =====
    start_tray_if_supported(&rt, &state, app.as_weak());
    install_close_handler(&app, state.clone());

    app.run()?;
    Ok(())
}

// ============================================================================
// Bindings des callbacks UI
// ============================================================================

fn bind_restart_button(app: &MainWindow, state: &Arc<AppState>, rt: tokio::runtime::Handle) {
    let app_weak = app.as_weak();
    let state = state.clone();
    app.on_restart_gateway(move || {
        let app_weak = app_weak.clone();
        let state = state.clone();
        let rt2 = rt.clone();
        rt.spawn(async move {
            // Détermine le mode depuis la config (Aggressive si activé, sinon Safe)
            let mode = if state.config.lock().await.auto_aggressive {
                RestartMode::Aggressive
            } else {
                RestartMode::Safe
            };
            push_log(
                &state,
                &app_weak,
                "info",
                format!("restart manuel ({mode:?})"),
            );
            set_restart_in_progress(&app_weak, true);
            let cli = state.cli.clone();
            let result = rt2
                .spawn(async move { restart_with_escalation(&cli, mode).await })
                .await;
            set_restart_in_progress(&app_weak, false);
            match result {
                Ok(Ok(report)) => {
                    let level = if report.final_ok { "info" } else { "warn" };
                    let summary = report
                        .steps
                        .iter()
                        .map(|s| {
                            format!(
                                "{}={}",
                                s.action,
                                if s.probe_after_ok { "ok" } else { "ko" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" → ");
                    push_log(
                        &state,
                        &app_weak,
                        level,
                        format!(
                            "restart {} : {}",
                            if report.final_ok { "OK" } else { "ÉCHEC" },
                            summary
                        ),
                    );
                }
                Ok(Err(e)) => {
                    push_log(&state, &app_weak, "error", format!("restart erreur : {e}"));
                }
                Err(join_err) => {
                    push_log(
                        &state,
                        &app_weak,
                        "error",
                        format!("restart task panic : {join_err}"),
                    );
                }
            }
        });
    });
}

fn bind_auto_restart_toggle(app: &MainWindow, state: &Arc<AppState>, rt: tokio::runtime::Handle) {
    let app_weak = app.as_weak();
    let state = state.clone();
    app.on_auto_restart_toggled(move |checked| {
        let app_weak = app_weak.clone();
        let state = state.clone();
        let rt = rt.clone();
        let rt_for_spawn = rt.clone();
        refresh_tray_auto_restart(&rt, &state, checked);
        rt.spawn(async move {
            {
                let mut cfg = state.config.lock().await;
                cfg.auto_restart_enabled = checked;
                if !checked {
                    cfg.auto_aggressive = false;
                }
                cfg.save_best_effort();
            }
            if checked {
                spawn_or_replace_watchdog(&rt_for_spawn, &state, app_weak.clone());
                push_log(&state, &app_weak, "info", "watchdog activé".into());
            } else {
                stop_watchdog(&state).await;
                push_log(&state, &app_weak, "info", "watchdog désactivé".into());
            }
        });
    });
}

fn bind_auto_aggressive_toggle(
    app: &MainWindow,
    state: &Arc<AppState>,
    rt: tokio::runtime::Handle,
) {
    let app_weak = app.as_weak();
    let state = state.clone();
    app.on_auto_aggressive_toggled(move |checked| {
        let app_weak = app_weak.clone();
        let state = state.clone();
        let rt2 = rt.clone();
        rt.spawn(async move {
            {
                let mut cfg = state.config.lock().await;
                cfg.auto_aggressive = checked;
                cfg.save_best_effort();
            }
            // Si le watchdog tourne, on le respawn avec la nouvelle config
            let auto_restart = state.config.lock().await.auto_restart_enabled;
            if auto_restart {
                spawn_or_replace_watchdog(&rt2, &state, app_weak.clone());
                push_log(
                    &state,
                    &app_weak,
                    "info",
                    format!(
                        "mode watchdog : {}",
                        if checked { "Aggressive" } else { "Safe" }
                    ),
                );
            }
        });
    });
}

fn bind_notifications_toggle(app: &MainWindow, state: &Arc<AppState>, rt: tokio::runtime::Handle) {
    let app_weak = app.as_weak();
    let state = state.clone();
    app.on_notifications_toggled(move |checked| {
        let app_weak = app_weak.clone();
        let state = state.clone();
        rt.spawn(async move {
            {
                let mut cfg = state.config.lock().await;
                cfg.notifications_enabled = checked;
                cfg.save_best_effort();
            }
            push_log(
                &state,
                &app_weak,
                "info",
                format!(
                    "notifications : {}",
                    if checked { "activées" } else { "désactivées" }
                ),
            );
            if checked {
                // Notification de feedback pour confirmer que ça marche
                notify::show(
                    notify::Level::Info,
                    "Notifications activées · ClawAgentMonitor vous préviendra des incidents watchdog.",
                );
            }
        });
    });
}

fn bind_start_at_login_toggle(app: &MainWindow, state: &Arc<AppState>) {
    let app_weak = app.as_weak();
    let state = state.clone();
    app.on_start_at_login_toggled(move |checked| {
        // Pas besoin du runtime tokio pour ce toggle : c'est de l'IO synchrone légère.
        match autostart::set_enabled(checked) {
            Ok(()) => {
                let state = state.clone();
                let app_weak = app_weak.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    rt.block_on(async {
                        let mut cfg = state.config.lock().await;
                        cfg.start_at_login = checked;
                        cfg.save_best_effort();
                    });
                    push_log_sync(
                        &state,
                        &app_weak,
                        "info",
                        format!(
                            "démarrage au login : {}",
                            if checked { "activé" } else { "désactivé" }
                        ),
                    );
                });
            }
            Err(e) => {
                let state = state.clone();
                let app_weak = app_weak.clone();
                push_log_sync(
                    &state,
                    &app_weak,
                    "error",
                    format!("autostart erreur : {e}"),
                );
                if let Some(app) = app_weak.upgrade() {
                    app.set_start_at_login_enabled(!checked);
                }
            }
        }
    });
}

// ============================================================================
// Watchdog : start / stop / event consumer
// ============================================================================

fn spawn_or_replace_watchdog(
    rt: &tokio::runtime::Handle,
    state: &Arc<AppState>,
    app_weak: slint::Weak<MainWindow>,
) {
    let state2 = state.clone();
    let rt2 = rt.clone();
    let app_weak2 = app_weak.clone();
    rt.spawn(async move {
        let cfg_snapshot = state2.config.lock().await.clone();
        let mode = if cfg_snapshot.auto_aggressive {
            RestartMode::Aggressive
        } else {
            RestartMode::Safe
        };
        let wd_cfg = WatchdogConfig {
            interval: Duration::from_secs(cfg_snapshot.watchdog_interval_secs),
            mode,
            ..WatchdogConfig::default()
        };
        // Stop l'ancien (si présent) avant d'en spawner un nouveau
        {
            let mut handle_slot = state2.watchdog_handle.lock().await;
            if let Some(h) = handle_slot.take() {
                h.cancel();
            }
        }
        let (handle, mut events) = spawn_watchdog((*state2.cli).clone(), wd_cfg);
        {
            let mut handle_slot = state2.watchdog_handle.lock().await;
            *handle_slot = Some(handle);
        }
        // Consommateur d'événements : alimente le journal de la GUI + notifs desktop
        let state_consumer = state2.clone();
        let app_weak_consumer = app_weak2.clone();
        rt2.spawn(async move {
            while let Some(ev) = events.recv().await {
                let (level, msg) = format_event(&ev);
                push_log(&state_consumer, &app_weak_consumer, level, msg.clone());
                // Notification desktop (best-effort, gardée par config)
                if state_consumer.config.lock().await.notifications_enabled {
                    if let Some(notif_level) = should_notify(&ev) {
                        notify::show(notif_level, &msg);
                    }
                }
            }
        });
    });
}

async fn stop_watchdog(state: &Arc<AppState>) {
    let mut slot = state.watchdog_handle.lock().await;
    if let Some(h) = slot.take() {
        h.cancel();
    }
}

fn format_event(ev: &WatchdogEvent) -> (&'static str, String) {
    match ev {
        WatchdogEvent::Started => ("info", "watchdog démarré".into()),
        WatchdogEvent::Stopped => ("info", "watchdog arrêté".into()),
        WatchdogEvent::Probe { ok, .. } => (
            if *ok { "info" } else { "warn" },
            format!("probe gateway : {}", if *ok { "OK" } else { "KO" }),
        ),
        WatchdogEvent::RestartAttempted {
            report_summary,
            final_ok,
        } => (
            if *final_ok { "info" } else { "warn" },
            format!(
                "restart auto {} : {}",
                if *final_ok { "OK" } else { "ÉCHEC" },
                report_summary
            ),
        ),
        WatchdogEvent::CrashLoopPause { .. } => (
            "warn",
            "crash-loop détecté, watchdog en pause 10 min".into(),
        ),
        WatchdogEvent::Error { message } => ("error", format!("erreur : {message}")),
    }
}

/// Décide si un évènement watchdog mérite une notification desktop.
/// On filtre volontairement les `Probe` (toutes les 5 min, déjà visibles
/// dans le journal) pour ne pas spammer l'utilisateur.
fn should_notify(ev: &WatchdogEvent) -> Option<notify::Level> {
    match ev {
        WatchdogEvent::Probe { .. } => None,
        WatchdogEvent::Started | WatchdogEvent::Stopped => None,
        WatchdogEvent::RestartAttempted { final_ok, .. } => Some(if *final_ok {
            notify::Level::Info
        } else {
            notify::Level::Warn
        }),
        WatchdogEvent::CrashLoopPause { .. } => Some(notify::Level::Warn),
        WatchdogEvent::Error { .. } => Some(notify::Level::Error),
    }
}

// ============================================================================
// Polling périodique (status_all + tasks_list → snapshot)
// ============================================================================

async fn poll_and_push(
    cli: &ClawCli,
    app_weak: &slint::Weak<MainWindow>,
    rt_handle: &tokio::runtime::Handle,
    state: &Arc<AppState>,
) {
    let started = std::time::Instant::now();
    let (status_res, tasks_res) =
        tokio::join!(cli.status_all(), cli.tasks_list(None, Some("running")),);

    let snap = match (status_res, tasks_res) {
        (Ok(s), Ok(t)) => Some(build_snapshot(&s, Some(&t))),
        (Ok(s), Err(e)) => {
            tracing::warn!(error = %e, "tasks_list failed, snapshot incomplete");
            Some(build_snapshot(&s, None))
        }
        (Err(e), _) => {
            tracing::warn!(error = %e, "status_all failed, skipping update");
            None
        }
    };

    if let Some(snap) = snap {
        tracing::info!(
            agents = snap.agents.len(),
            running = snap.total_running_tasks,
            gateway = ?snap.gateway,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "snapshot pushed to UI"
        );
        refresh_tray_from_snapshot(rt_handle, state, &snap);
        push_snapshot_to_ui(app_weak, snap);
    }
}

fn push_snapshot_to_ui(app_weak: &slint::Weak<MainWindow>, snap: StatusSnapshot) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(app) = app_weak.upgrade() else {
            return;
        };
        let agents_ui: Vec<AgentInfo> = snap.agents.iter().map(agent_to_ui).collect();
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

// ============================================================================
// Journal watchdog (côté GUI)
// ============================================================================

/// Pousse une entrée de journal depuis un contexte async. Conserve les
/// LOG_MAX_ENTRIES dernières et reflète l'ensemble vers la GUI.
fn push_log(
    state: &Arc<AppState>,
    app_weak: &slint::Weak<MainWindow>,
    level: &str,
    message: String,
) {
    let entry = LogEntry {
        ts: SharedString::from(now_hms()),
        level: SharedString::from(level),
        message: SharedString::from(message),
    };
    let state2 = state.clone();
    let app_weak2 = app_weak.clone();
    tokio::spawn(async move {
        let mut log = state2.log.lock().await;
        log.push_front(entry);
        while log.len() > LOG_MAX_ENTRIES {
            log.pop_back();
        }
        let snapshot: Vec<LogEntry> = log.iter().cloned().collect();
        drop(log);
        let app_weak2 = app_weak2.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(app) = app_weak2.upgrade() {
                app.set_watchdog_log(ModelRc::new(VecModel::from(snapshot)));
            }
        });
    });
}

/// Variante synchrone pour les contextes non-tokio (callbacks UI très simples).
fn push_log_sync(
    state: &Arc<AppState>,
    app_weak: &slint::Weak<MainWindow>,
    level: &str,
    message: String,
) {
    let entry = LogEntry {
        ts: SharedString::from(now_hms()),
        level: SharedString::from(level),
        message: SharedString::from(message),
    };
    let state2 = state.clone();
    let app_weak2 = app_weak.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut log = state2.log.lock().await;
            log.push_front(entry);
            while log.len() > LOG_MAX_ENTRIES {
                log.pop_back();
            }
            let snapshot: Vec<LogEntry> = log.iter().cloned().collect();
            drop(log);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(app) = app_weak2.upgrade() {
                    app.set_watchdog_log(ModelRc::new(VecModel::from(snapshot)));
                }
            });
        });
    });
}

// ============================================================================
// Tray icon (Linux only) + close-to-tray
// ============================================================================

#[cfg(target_os = "linux")]
fn start_tray_if_supported(
    rt: &tokio::runtime::Runtime,
    state: &Arc<AppState>,
    app_weak: slint::Weak<MainWindow>,
) {
    let state = state.clone();
    rt.spawn(async move {
        match tray::start().await {
            Ok((handle, mut rx)) => {
                tracing::info!("tray icon started (ksni)");
                {
                    let mut slot = state.tray_handle.lock().await;
                    *slot = Some(handle);
                }
                // Consume tray commands
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        tray::TrayCommand::ShowWindow => {
                            let weak = app_weak.clone();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(app) = weak.upgrade() {
                                    let _ = app.window().show();
                                }
                            });
                        }
                        tray::TrayCommand::Quit => {
                            let _ = slint::invoke_from_event_loop(|| {
                                let _ = slint::quit_event_loop();
                            });
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "tray icon could not start, continuing without");
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
fn start_tray_if_supported(
    _rt: &tokio::runtime::Runtime,
    _state: &Arc<AppState>,
    _app_weak: slint::Weak<MainWindow>,
) {
    // pas de tray cross-platform pour l'instant
}

/// Au lieu de quitter quand on ferme la fenêtre, on la cache si le tray est
/// disponible (sinon comportement par défaut : quit_event_loop).
fn install_close_handler(app: &MainWindow, state: Arc<AppState>) {
    app.window().on_close_requested(move || {
        if has_active_tray(&state) {
            tracing::info!("close requested, hiding window (tray is active)");
            CloseRequestResponse::HideWindow
        } else {
            tracing::info!("close requested, no tray active, quitting app");
            let _ = slint::quit_event_loop();
            CloseRequestResponse::HideWindow
        }
    });
}

#[cfg(target_os = "linux")]
fn has_active_tray(state: &Arc<AppState>) -> bool {
    // try_lock : si le mutex est libre, on regarde ; sinon on assume oui
    // (c'est juste un best-effort pour décider entre hide et quit).
    match state.tray_handle.try_lock() {
        Ok(slot) => slot.as_ref().map(|h| !h.is_closed()).unwrap_or(false),
        Err(_) => true,
    }
}

#[cfg(not(target_os = "linux"))]
fn has_active_tray(_state: &Arc<AppState>) -> bool {
    false
}

/// Pousse les nouveautés du snapshot vers le tray (icône + tooltip).
fn refresh_tray_from_snapshot(
    rt: &tokio::runtime::Handle,
    state: &Arc<AppState>,
    snap: &StatusSnapshot,
) {
    let _ = (rt, state, snap);
    #[cfg(target_os = "linux")]
    {
        let state = state.clone();
        let gw = match snap.gateway {
            claw_core::agent_state::GatewayState::Up => tray::GatewayKind::Up,
            claw_core::agent_state::GatewayState::Down => tray::GatewayKind::Down,
            claw_core::agent_state::GatewayState::Unknown => tray::GatewayKind::Unknown,
        };
        let agents_running = snap.total_running_tasks as u32;
        rt.spawn(async move {
            let slot = state.tray_handle.lock().await;
            if let Some(handle) = slot.as_ref() {
                tray::update(handle, Some(gw), None, Some(agents_running)).await;
            }
        });
    }
}

/// Pousse l'état d'auto-restart au tray (depuis les bind_*_toggled).
fn refresh_tray_auto_restart(
    rt: &tokio::runtime::Handle,
    state: &Arc<AppState>,
    auto_restart: bool,
) {
    let _ = (rt, state, auto_restart);
    #[cfg(target_os = "linux")]
    {
        let state = state.clone();
        rt.spawn(async move {
            let slot = state.tray_handle.lock().await;
            if let Some(handle) = slot.as_ref() {
                tray::update(handle, None, Some(auto_restart), None).await;
            }
        });
    }
}

// ============================================================================
// Helpers de formatage
// ============================================================================

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
