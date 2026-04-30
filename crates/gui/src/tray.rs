//! Icône système (StatusNotifierItem) pour Linux via `ksni`.
//!
//! Activée uniquement sous Linux. Sur les autres plateformes, le module
//! expose une API stub no-op pour que le code appelant n'ait pas à
//! `cfg(target_os)` ses appels.
//!
//! Comportement :
//! - clic gauche sur l'icône : réaffiche la fenêtre Slint
//! - menu : "Afficher" / "Auto-restart" (informatif, lecture seule) / "Quitter"
//! - l'icône change selon l'état du gateway (UP / DOWN / Unknown)
//!
//! Communication : le tray vit dans son propre task tokio. Il publie ses
//! évènements via un `mpsc::Sender<TrayCommand>` consommé par le thread
//! principal. C'est ce dernier qui exécute les actions sur la GUI Slint
//! (qui exige le main thread).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ShowWindow,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayKind {
    Up,
    Down,
    Unknown,
}

// =============================================================================
// Implémentation Linux (ksni)
// =============================================================================
#[cfg(target_os = "linux")]
mod imp {
    use super::{GatewayKind, TrayCommand};
    use anyhow::{Context, Result};
    use ksni::{
        menu::{MenuItem, StandardItem},
        Handle, Tray, TrayMethods,
    };
    use tokio::sync::mpsc;

    /// État interne du tray. Mutable via `Handle::update`.
    pub struct ClawTray {
        gateway: GatewayKind,
        auto_restart: bool,
        agents_running: u32,
        tx: mpsc::UnboundedSender<TrayCommand>,
    }

    impl ClawTray {
        fn new(tx: mpsc::UnboundedSender<TrayCommand>) -> Self {
            Self {
                gateway: GatewayKind::Unknown,
                auto_restart: false,
                agents_running: 0,
                tx,
            }
        }
    }

    impl Tray for ClawTray {
        fn id(&self) -> String {
            "clawagentmonitor".into()
        }

        fn title(&self) -> String {
            match self.gateway {
                GatewayKind::Up => format!(
                    "ClawAgentMonitor — Gateway UP, {} en cours",
                    self.agents_running
                ),
                GatewayKind::Down => "ClawAgentMonitor — Gateway DOWN".into(),
                GatewayKind::Unknown => "ClawAgentMonitor".into(),
            }
        }

        fn icon_name(&self) -> String {
            // Noms FreeDesktop disponibles sur la plupart des thèmes Plasma/GNOME/Adwaita
            match self.gateway {
                GatewayKind::Up => "network-transmit-receive".into(),
                GatewayKind::Down => "network-error".into(),
                GatewayKind::Unknown => "network-offline".into(),
            }
        }

        // Clic gauche
        fn activate(&mut self, _x: i32, _y: i32) {
            let _ = self.tx.send(TrayCommand::ShowWindow);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            vec![
                StandardItem {
                    label: "Afficher".into(),
                    icon_name: "window".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.tx.send(TrayCommand::ShowWindow);
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                // Élément informatif (lecture seule pour cette première version
                // du tray ; le toggle se fait dans la fenêtre principale).
                StandardItem {
                    label: format!(
                        "Auto-restart : {}",
                        if self.auto_restart { "ON" } else { "off" }
                    ),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: format!(
                        "Gateway : {}",
                        match self.gateway {
                            GatewayKind::Up => "UP",
                            GatewayKind::Down => "DOWN",
                            GatewayKind::Unknown => "?",
                        }
                    ),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Quitter".into(),
                    icon_name: "application-exit".into(),
                    activate: Box::new(|this: &mut Self| {
                        let _ = this.tx.send(TrayCommand::Quit);
                    }),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    /// Démarre le tray ksni dans le runtime tokio courant.
    /// Retourne `(handle, rx)` où `rx` reçoit les commandes utilisateur.
    pub async fn start() -> Result<(Handle<ClawTray>, mpsc::UnboundedReceiver<TrayCommand>)> {
        let (tx, rx) = mpsc::unbounded_channel();
        let tray = ClawTray::new(tx);
        let handle = tray
            .spawn()
            .await
            .context("tray spawn failed (StatusNotifierWatcher injoignable ?)")?;
        Ok((handle, rx))
    }

    /// Met à jour l'état affiché par le tray (icône, titre, infos menu).
    pub async fn update(
        handle: &Handle<ClawTray>,
        gateway: Option<GatewayKind>,
        auto_restart: Option<bool>,
        agents_running: Option<u32>,
    ) {
        let _ = handle
            .update(|t| {
                if let Some(gw) = gateway {
                    t.gateway = gw;
                }
                if let Some(ar) = auto_restart {
                    t.auto_restart = ar;
                }
                if let Some(n) = agents_running {
                    t.agents_running = n;
                }
            })
            .await;
    }
}

#[cfg(target_os = "linux")]
pub use imp::{start, update, ClawTray};

// =============================================================================
// Stub no-op pour les autres OS (à remplacer par tray-icon plus tard)
// =============================================================================
#[cfg(not(target_os = "linux"))]
mod imp {
    use super::TrayCommand;
    use anyhow::{anyhow, Result};
    use tokio::sync::mpsc;

    /// Type opaque (jamais instancié sur les non-Linux).
    pub struct ClawTray;

    pub async fn start() -> Result<(ClawTray, mpsc::UnboundedReceiver<TrayCommand>)> {
        Err(anyhow!("tray icon not implemented on this OS yet"))
    }

    pub async fn update(
        _handle: &ClawTray,
        _gateway: Option<super::GatewayKind>,
        _auto_restart: Option<bool>,
        _agents_running: Option<u32>,
    ) {
    }
}

#[cfg(not(target_os = "linux"))]
pub use imp::{start, update, ClawTray};
