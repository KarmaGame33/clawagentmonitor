//! Example end-to-end : appelle vraiment `openclaw status --all --json`,
//! puis imprime le snapshot calculé par `agent_state::build_snapshot`.
//!
//! Lancer avec : `cargo run -p claw-core --example snapshot`

use anyhow::Result;
use claw_core::agent_state::build_snapshot;
use claw_core::cli::ClawCli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = ClawCli::new();

    println!("== Probe gateway ==");
    match cli.gateway_probe().await {
        Ok(p) => println!("  ok={}, capability={:?}", p.ok, p.capability),
        Err(e) => println!("  probe error: {e}"),
    }

    println!();
    println!("== Status all ==");
    let status = cli.status_all().await?;
    println!("  runtimeVersion: {:?}", status.runtime_version);
    println!("  total tasks: {}", status.tasks.total);
    println!("  running tasks: {}", status.tasks.by_status.running);
    println!("  failures: {}", status.tasks.by_status.failed);
    println!("  agents in heartbeat: {}", status.heartbeat.agents.len());
    if let Some(g) = &status.gateway {
        println!("  gateway.reachable: {}", g.reachable);
    } else {
        println!("  gateway: <down or unknown>");
    }

    println!();
    println!("== Tasks list (running only) ==");
    let tl = cli.tasks_list(None, Some("running")).await?;
    println!("  count: {}", tl.count);
    for t in tl.tasks.iter().take(10) {
        println!(
            "  - [{} | {}] agent={:?} label={:?}",
            t.status,
            t.runtime,
            t.agent_id_hint(),
            t.label
        );
    }

    println!();
    println!("== Snapshot calculé ==");
    let snap = build_snapshot(&status, Some(&tl));
    println!(
        "  Gateway state: {:?} ({})",
        snap.gateway,
        snap.gateway_runtime_short.as_deref().unwrap_or("-")
    );
    println!("  Total running: {}", snap.total_running_tasks);
    println!("  Stale running: {}", snap.stale_running);
    println!();
    for a in &snap.agents {
        println!(
            "  [{:6}] {:8} {:<8} | {} (running={}, sessions={}, last={:?}ms)",
            a.status.as_str(),
            a.id,
            a.name,
            a.note,
            a.running_tasks,
            a.sessions_count,
            a.last_active_age_ms,
        );
    }

    Ok(())
}
