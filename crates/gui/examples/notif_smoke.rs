fn main() {
    notify_rust::Notification::new()
        .summary("ClawAgentMonitor")
        .body("Test de notification depuis ClawAgentMonitor")
        .icon("dialog-information")
        .show()
        .map(|_| println!("notification envoyée"))
        .unwrap_or_else(|e| println!("notif failed: {e}"));
}
