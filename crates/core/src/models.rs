//! Structs serde miroir des sorties JSON du CLI `openclaw`.
//!
//! Toutes les structures sont **tolérantes** : champs optionnels, defaults
//! systématiques. Ainsi le code fonctionne aussi bien quand le gateway est
//! up (sortie riche avec blocs `agents`, `gateway`, `gatewayService`) que
//! quand il est down (sortie minimale avec `heartbeat`, `tasks`, `sessions`).

use serde::Deserialize;

/// Statut global retourné par `openclaw status --all --json`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusAll {
    pub runtime_version: Option<String>,
    pub heartbeat: HeartbeatBlock,
    pub tasks: TasksSummary,
    pub task_audit: TaskAudit,
    pub sessions: SessionsBlock,
    pub agents: Option<AgentsBlock>,
    pub gateway: Option<GatewayInfo>,
    pub gateway_service: Option<GatewayService>,
    pub channel_summary: serde_json::Value,
    pub security_audit: Option<SecurityAudit>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeartbeatBlock {
    pub default_agent_id: Option<String>,
    pub agents: Vec<HeartbeatAgent>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeartbeatAgent {
    pub agent_id: String,
    pub enabled: bool,
    pub every: Option<String>,
    pub every_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TasksSummary {
    pub total: u64,
    pub active: u64,
    pub terminal: u64,
    pub failures: u64,
    pub by_status: TasksByStatus,
    pub by_runtime: TasksByRuntime,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TasksByStatus {
    pub queued: u64,
    pub running: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub cancelled: u64,
    pub lost: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TasksByRuntime {
    pub subagent: u64,
    pub acp: u64,
    pub cli: u64,
    pub cron: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskAudit {
    pub total: u64,
    pub warnings: u64,
    pub errors: u64,
    pub by_code: TaskAuditByCode,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskAuditByCode {
    pub stale_queued: u64,
    pub stale_running: u64,
    pub lost: u64,
    pub delivery_failed: u64,
    pub missing_cleanup: u64,
    pub inconsistent_timestamps: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionsBlock {
    pub paths: Vec<String>,
    pub count: u64,
    pub active_minutes: Option<u64>,
    pub recent: Vec<SessionRecent>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionRecent {
    pub agent_id: String,
    pub key: String,
    pub kind: Option<String>,
    pub session_id: Option<String>,
    pub updated_at: i64,
    pub age: i64,
    pub aborted_last_run: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub remaining_tokens: Option<u64>,
    pub percent_used: Option<u32>,
    pub model: Option<String>,
    pub context_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentsBlock {
    pub default_id: Option<String>,
    pub agents: Vec<AgentEntry>,
    pub total_sessions: u64,
    pub bootstrap_pending_count: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentEntry {
    pub id: String,
    pub name: Option<String>,
    pub workspace_dir: Option<String>,
    pub bootstrap_pending: bool,
    pub sessions_path: Option<String>,
    pub sessions_count: u64,
    pub last_updated_at: Option<i64>,
    pub last_active_age_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewayInfo {
    pub mode: Option<String>,
    pub url: Option<String>,
    pub url_source: Option<String>,
    pub misconfigured: bool,
    pub reachable: bool,
    pub connect_latency_ms: Option<u64>,
    #[serde(rename = "self")]
    pub self_info: Option<GatewaySelf>,
    pub error: Option<String>,
    pub auth_warning: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewaySelf {
    pub host: Option<String>,
    pub ip: Option<String>,
    pub version: Option<String>,
    pub platform: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewayService {
    pub label: Option<String>,
    pub installed: bool,
    pub loaded: bool,
    pub managed_by_open_claw: bool,
    pub externally_managed: bool,
    pub loaded_text: Option<String>,
    pub runtime: Option<GatewayServiceRuntime>,
    pub runtime_short: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewayServiceRuntime {
    pub status: Option<String>,
    pub state: Option<String>,
    pub sub_state: Option<String>,
    pub pid: Option<u32>,
    pub last_exit_status: Option<i32>,
    pub last_exit_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SecurityAudit {
    pub ts: i64,
    pub summary: SecurityAuditSummary,
    pub findings: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SecurityAuditSummary {
    pub critical: u64,
    pub warn: u64,
    pub info: u64,
}

// =============================================================================
// Sortie de `openclaw tasks list --json`
// =============================================================================

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TasksList {
    pub count: u64,
    pub runtime: Option<String>,
    pub status: Option<String>,
    pub tasks: Vec<TaskEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskEntry {
    pub task_id: String,
    pub runtime: String,
    pub source_id: Option<String>,
    pub requester_session_key: Option<String>,
    pub owner_key: Option<String>,
    pub scope_kind: Option<String>,
    pub child_session_key: Option<String>,
    pub parent_flow_id: Option<String>,
    pub run_id: Option<String>,
    pub label: Option<String>,
    pub task: Option<String>,
    pub status: String,
    pub delivery_status: Option<String>,
    pub notify_policy: Option<String>,
    pub created_at: Option<i64>,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub last_event_at: Option<i64>,
    pub cleanup_after: Option<i64>,
    pub progress_summary: Option<String>,
    pub terminal_summary: Option<String>,
    pub error: Option<String>,
}

impl TaskEntry {
    /// Retourne l'agentId déduit du sessionKey (`agent:<id>:...`).
    pub fn agent_id_hint(&self) -> Option<&str> {
        self.owner_key
            .as_deref()
            .or(self.requester_session_key.as_deref())
            .or(self.child_session_key.as_deref())
            .and_then(|k| k.split(':').nth(1))
    }
}

// =============================================================================
// Sortie de `openclaw gateway probe --json`
// =============================================================================

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GatewayProbe {
    pub ok: bool,
    pub degraded: bool,
    pub capability: Option<String>,
    pub ts: Option<i64>,
    pub duration_ms: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub primary_target_id: Option<String>,
    pub warnings: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS_UP: &str = include_str!("../tests/fixtures/status_all_up.json");
    const TASKS_LIST: &str = include_str!("../tests/fixtures/tasks_list.json");
    const GATEWAY_PROBE: &str = include_str!("../tests/fixtures/gateway_probe.json");

    #[test]
    fn parse_status_all_up() {
        let s: StatusAll = serde_json::from_str(STATUS_UP).expect("parse status_all");
        assert_eq!(s.runtime_version.as_deref(), Some("2026.4.26"));
        assert!(s.tasks.total > 0);
        assert!(!s.heartbeat.agents.is_empty());
        let agents = s.agents.expect("agents block present when gateway up");
        assert_eq!(agents.agents.len(), 5);
        let gw = s.gateway.expect("gateway block present when up");
        assert!(gw.reachable);
    }

    #[test]
    fn parse_tasks_list() {
        let t: TasksList = serde_json::from_str(TASKS_LIST).expect("parse tasks_list");
        assert!(t.count > 0);
        assert!(!t.tasks.is_empty());
        let first = &t.tasks[0];
        assert!(matches!(
            first.runtime.as_str(),
            "subagent" | "cli" | "cron" | "acp"
        ));
    }

    #[test]
    fn parse_gateway_probe() {
        let p: GatewayProbe = serde_json::from_str(GATEWAY_PROBE).expect("parse probe");
        assert!(p.ok);
    }

    #[test]
    fn agent_id_hint_extraction() {
        let t = TaskEntry {
            owner_key: Some("agent:ada:subagent:foo".into()),
            ..TaskEntry::default()
        };
        assert_eq!(t.agent_id_hint(), Some("ada"));
    }
}
