use anyhow::Result;
use slint::{ModelRc, VecModel};

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

    let app = MainWindow::new()?;

    let agents = VecModel::from(vec![
        AgentInfo {
            id: "main".into(),
            name: "Hex".into(),
            model: "deepseek-v4-pro:cloud".into(),
            status: "green".into(),
        },
        AgentInfo {
            id: "ada".into(),
            name: "Ada".into(),
            model: "kimi-k2.6:cloud".into(),
            status: "orange".into(),
        },
        AgentInfo {
            id: "matisse".into(),
            name: "Matisse".into(),
            model: "kimi-k2.6:cloud".into(),
            status: "green".into(),
        },
        AgentInfo {
            id: "fixit".into(),
            name: "Fixit".into(),
            model: "MiniMax-M2.7:cloud".into(),
            status: "red".into(),
        },
        AgentInfo {
            id: "critix".into(),
            name: "Critix".into(),
            model: "glm-5.1:cloud".into(),
            status: "gray".into(),
        },
    ]);

    app.set_agents(ModelRc::new(agents));
    app.set_gateway_status("down".into());

    app.on_restart_gateway(|| {
        tracing::info!("Restart gateway: not implemented yet (scaffolding stage)");
    });

    app.run()?;
    Ok(())
}
