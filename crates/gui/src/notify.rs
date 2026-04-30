//! Wrapper minimaliste autour de `notify-rust` pour les notifications desktop.
//!
//! Volontairement « best-effort » : si l'environnement utilisateur ne supporte
//! pas les notifications (pas de daemon dbus, environnement minimal, etc.),
//! l'erreur est juste loguée — on ne fait jamais paniquer la GUI.

use notify_rust::Notification;

/// Niveau d'importance, utilisé pour choisir l'icône XDG.
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn xdg_icon(&self) -> &'static str {
        match self {
            Level::Info => "dialog-information",
            Level::Warn => "dialog-warning",
            Level::Error => "dialog-error",
        }
    }
}

pub fn show(level: Level, body: &str) {
    let res = Notification::new()
        .summary("ClawAgentMonitor")
        .body(body)
        .icon(level.xdg_icon())
        .appname("ClawAgentMonitor")
        .show();
    if let Err(e) = res {
        tracing::debug!(error = %e, "notify-rust show failed (best-effort)");
    }
}
