//! Wrapper cross-platform pour l'auto-démarrage au login.
//!
//! Délègue à la crate [`auto_launch`] qui gère :
//! - Linux : fichier `.desktop` dans `~/.config/autostart/`
//! - macOS : `LaunchAgent` `.plist` dans `~/Library/LaunchAgents/`
//! - Windows : entrée dans le registre `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`

use anyhow::{Context, Result};
use auto_launch::AutoLaunchBuilder;

const APP_NAME: &str = "ClawAgentMonitor";

fn builder() -> Result<auto_launch::AutoLaunch> {
    let exe = std::env::current_exe().context("could not resolve current exe path")?;
    let exe_str = exe
        .to_str()
        .context("exe path is not valid UTF-8")?
        .to_string();
    // Note: sur macOS, `auto_launch` accepte aussi `set_macos_launch_mode`
    // (LaunchAgent vs login items). On garde le mode par défaut ici, le
    // packaging macOS sera traité dans une étape dédiée.
    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe_str)
        .build()
        .context("auto_launch builder failed")
}

pub fn is_enabled() -> Result<bool> {
    let al = builder()?;
    al.is_enabled().context("auto_launch::is_enabled failed")
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let al = builder()?;
    if enabled {
        al.enable().context("auto_launch::enable failed")
    } else {
        al.disable().context("auto_launch::disable failed")
    }
}
