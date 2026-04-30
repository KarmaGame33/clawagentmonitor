//! Heuristique d'état des agents et du gateway.
//!
//! Couleurs (alignées avec le pictogramme dans la GUI Slint) :
//! - **Green** : agent OK, aucune tâche en cours, pas d'erreur récente.
//! - **Orange** : tâche en cours pour cet agent (subagent/cli/cron/acp running).
//! - **Red** : `taskAudit.stale_running` ou `abortedLastRun` ou erreur récente.
//! - **Gray** : agent désactivé / sans activité / hors heartbeat.

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::models::{StatusAll, TasksList};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Green,
    Orange,
    Red,
    Gray,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Green => "green",
            Status::Orange => "orange",
            Status::Red => "red",
            Status::Gray => "gray",
        }
    }
}

/// Statut d'un agent prêt à être affiché dans la GUI.
#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub id: String,
    pub name: String,
    pub model: Option<String>,
    pub status: Status,
    /// Texte court explicatif (ex: "tâche en cours", "session annulée").
    pub note: String,
    /// Activité récente (en millisecondes), si connue.
    pub last_active_age_ms: Option<i64>,
    pub sessions_count: u64,
    pub running_tasks: u32,
}

/// Statut global du gateway pour le bandeau d'en-tête.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayState {
    Up,
    Down,
    Unknown,
}

impl GatewayState {
    pub fn as_str(self) -> &'static str {
        match self {
            GatewayState::Up => "up",
            GatewayState::Down => "down",
            GatewayState::Unknown => "unknown",
        }
    }
}

/// Snapshot complet à pousser dans la GUI à chaque tick de polling.
#[derive(Debug, Clone, Serialize)]
pub struct StatusSnapshot {
    pub gateway: GatewayState,
    pub gateway_runtime_short: Option<String>,
    pub agents: Vec<AgentStatus>,
    pub total_running_tasks: u64,
    pub total_failures: u64,
    pub stale_running: u64,
}

/// Combine `status_all` + (optionnellement) `tasks list --status running` en un snapshot.
pub fn build_snapshot(status: &StatusAll, running_tasks: Option<&TasksList>) -> StatusSnapshot {
    // 1. Compter les tâches running par agent (via owner/requester sessionKey)
    let mut running_per_agent: HashMap<String, u32> = HashMap::new();
    if let Some(tl) = running_tasks {
        for t in tl.tasks.iter().filter(|t| t.status == "running") {
            if let Some(aid) = t.agent_id_hint() {
                *running_per_agent.entry(aid.to_string()).or_insert(0) += 1;
            }
        }
    }

    // 2. Détecter abortedLastRun par agent (depuis sessions.recent)
    let mut aborted: HashSet<String> = HashSet::new();
    let mut last_age_per_agent: HashMap<String, i64> = HashMap::new();
    let mut model_per_agent: HashMap<String, String> = HashMap::new();
    for s in &status.sessions.recent {
        if s.aborted_last_run {
            aborted.insert(s.agent_id.clone());
        }
        let entry = last_age_per_agent
            .entry(s.agent_id.clone())
            .or_insert(s.age);
        if s.age < *entry {
            *entry = s.age;
        }
        if let Some(m) = &s.model {
            model_per_agent
                .entry(s.agent_id.clone())
                .or_insert_with(|| m.clone());
        }
    }

    // 3. Liste de référence des agents : prioriser status.agents (riche), fallback heartbeat
    let agents: Vec<AgentStatus> = if let Some(ab) = &status.agents {
        ab.agents
            .iter()
            .map(|a| {
                let id = a.id.clone();
                let name = a.name.clone().unwrap_or_else(|| id.clone());
                let model = model_per_agent.get(&id).cloned();
                let last_age = a
                    .last_active_age_ms
                    .or_else(|| last_age_per_agent.get(&id).copied());
                let running = *running_per_agent.get(&id).unwrap_or(&0);
                let stale_for_agent = status.task_audit.by_code.stale_running > 0
                    && running == 0
                    && last_age.map(|a| a > 60_000).unwrap_or(false);
                let (status_color, note) = classify(
                    /* enabled */
                    status
                        .heartbeat
                        .agents
                        .iter()
                        .find(|h| h.agent_id == id)
                        .map(|h| h.enabled)
                        .unwrap_or(true),
                    a.sessions_count,
                    last_age,
                    aborted.contains(&id),
                    running,
                    stale_for_agent,
                );
                AgentStatus {
                    id,
                    name,
                    model,
                    status: status_color,
                    note,
                    last_active_age_ms: last_age,
                    sessions_count: a.sessions_count,
                    running_tasks: running,
                }
            })
            .collect()
    } else {
        // Mode dégradé : on n'a que heartbeat
        status
            .heartbeat
            .agents
            .iter()
            .map(|h| {
                let id = h.agent_id.clone();
                let last_age = last_age_per_agent.get(&id).copied();
                let running = *running_per_agent.get(&id).unwrap_or(&0);
                let (status_color, note) = classify(
                    h.enabled,
                    /* sessions_count */ 0,
                    last_age,
                    aborted.contains(&id),
                    running,
                    false,
                );
                AgentStatus {
                    id: id.clone(),
                    name: id,
                    model: model_per_agent.get(&h.agent_id).cloned(),
                    status: status_color,
                    note,
                    last_active_age_ms: last_age,
                    sessions_count: 0,
                    running_tasks: running,
                }
            })
            .collect()
    };

    let gateway = match &status.gateway {
        Some(g) if g.reachable => GatewayState::Up,
        Some(_) => GatewayState::Down,
        None => GatewayState::Unknown,
    };

    StatusSnapshot {
        gateway,
        gateway_runtime_short: status
            .gateway_service
            .as_ref()
            .and_then(|gs| gs.runtime_short.clone()),
        agents,
        total_running_tasks: status.tasks.by_status.running,
        total_failures: status.tasks.by_status.failed + status.tasks.by_status.timed_out,
        stale_running: status.task_audit.by_code.stale_running,
    }
}

/// Décide la couleur + note pour un agent.
fn classify(
    enabled: bool,
    sessions_count: u64,
    last_age_ms: Option<i64>,
    aborted: bool,
    running_tasks: u32,
    stale_running: bool,
) -> (Status, String) {
    if !enabled && sessions_count == 0 {
        return (Status::Gray, "désactivé".into());
    }
    if stale_running {
        return (Status::Red, "tâche bloquée détectée".into());
    }
    if aborted {
        return (Status::Red, "session annulée".into());
    }
    if running_tasks > 0 {
        return (
            Status::Orange,
            if running_tasks == 1 {
                "tâche en cours".into()
            } else {
                format!("{running_tasks} tâches en cours")
            },
        );
    }
    let age_min = last_age_ms.map(|m| m / 60_000).unwrap_or(i64::MAX);
    if age_min == i64::MAX {
        return (Status::Gray, "aucune activité connue".into());
    }
    if age_min > 60 {
        return (Status::Gray, format!("inactif depuis {age_min} min"));
    }
    (Status::Green, "ok".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS_UP: &str = include_str!("../tests/fixtures/status_all_up.json");
    const TASKS_LIST: &str = include_str!("../tests/fixtures/tasks_list.json");

    #[test]
    fn snapshot_with_real_status_up() {
        let status: StatusAll = serde_json::from_str(STATUS_UP).unwrap();
        let tl: TasksList = serde_json::from_str(TASKS_LIST).unwrap();
        let snap = build_snapshot(&status, Some(&tl));
        assert_eq!(snap.gateway, GatewayState::Up);
        assert_eq!(snap.agents.len(), 5);
        let ids: Vec<&str> = snap.agents.iter().map(|a| a.id.as_str()).collect();
        assert!(ids.contains(&"main"));
        assert!(ids.contains(&"ada"));
    }

    #[test]
    fn classify_disabled_agent_is_gray() {
        let (s, _) = classify(false, 0, None, false, 0, false);
        assert_eq!(s, Status::Gray);
    }

    #[test]
    fn classify_running_task_is_orange() {
        let (s, note) = classify(true, 5, Some(1000), false, 2, false);
        assert_eq!(s, Status::Orange);
        assert!(note.contains("2"));
    }

    #[test]
    fn classify_aborted_is_red() {
        let (s, _) = classify(true, 5, Some(1000), true, 0, false);
        assert_eq!(s, Status::Red);
    }

    #[test]
    fn classify_recent_activity_is_green() {
        let (s, _) = classify(true, 5, Some(5_000), false, 0, false);
        assert_eq!(s, Status::Green);
    }

    #[test]
    fn classify_old_activity_is_gray() {
        let (s, note) = classify(true, 5, Some(120 * 60_000), false, 0, false);
        assert_eq!(s, Status::Gray);
        assert!(note.contains("min"));
    }
}
