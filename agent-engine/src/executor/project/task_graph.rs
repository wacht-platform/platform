use super::core::AgentExecutor;
use commands::EnsureThreadTaskGraphCommand;
use common::error::AppError;
use queries::{ListReadyThreadTaskNodesQuery, ListThreadTaskEdgesQuery, ListThreadTaskNodesQuery};
use std::collections::{HashMap, HashSet};

impl AgentExecutor {
    pub(crate) fn invalidate_task_graph_snapshot(&mut self) {
        self.task_graph_snapshot = None;
    }

    pub(crate) fn render_task_graph_view(snapshot: &serde_json::Value) -> String {
        let graph_status = snapshot
            .get("graph")
            .and_then(|graph| graph.get("status"))
            .and_then(|status| status.as_str())
            .unwrap_or("unknown");

        let nodes = snapshot
            .get("nodes")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let edges = snapshot
            .get("edges")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let ready_node_ids = snapshot
            .get("ready_node_ids")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(|s| s.to_string()))
            .collect::<HashSet<_>>();

        let mut dependency_map: HashMap<String, Vec<String>> = HashMap::new();
        for edge in edges {
            let Some(to_node_id) = edge.get("to_node_id").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(from_node_id) = edge.get("from_node_id").and_then(|value| value.as_str())
            else {
                continue;
            };
            dependency_map
                .entry(to_node_id.to_string())
                .or_default()
                .push(from_node_id.to_string());
        }

        let mut lines = vec![format!("Graph status: {graph_status}")];

        let ready_lines = nodes
            .iter()
            .filter_map(|node| {
                let id = node.get("id")?.as_str()?;
                if !ready_node_ids.contains(id) {
                    return None;
                }
                let title = node
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Untitled");
                Some(format!("- {id} {title}"))
            })
            .collect::<Vec<_>>();

        if ready_lines.is_empty() {
            lines.push("Ready nodes: none".to_string());
        } else {
            lines.push("Ready nodes:".to_string());
            lines.extend(ready_lines);
        }

        lines.push("All nodes:".to_string());

        for node in nodes {
            let id = node
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let title = node
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or("Untitled");
            let status = node
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");

            lines.push(format!("- {id} {title} [{status}]"));

            if let Some(depends_on) = dependency_map.get(id) {
                if !depends_on.is_empty() {
                    lines.push(format!("  depends_on: {}", depends_on.join(", ")));
                }
            }
        }

        lines.join("\n")
    }

    pub(crate) async fn ensure_task_graph_snapshot(
        &mut self,
    ) -> Result<serde_json::Value, AppError> {
        if let Some(snapshot) = &self.task_graph_snapshot {
            let graph_status = snapshot
                .get("graph")
                .and_then(|graph| graph.get("status"))
                .and_then(|status| status.as_str());

            if !matches!(graph_status, Some("completed" | "failed" | "cancelled")) {
                return Ok(snapshot.clone());
            }

            self.task_graph_snapshot = None;
        }

        let mut ensure = EnsureThreadTaskGraphCommand::new(
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
        );
        if let Some(board_item_id) = self.current_board_item_id() {
            ensure = ensure.with_board_item_id(board_item_id);
        }
        let graph = ensure
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let nodes = ListThreadTaskNodesQuery::new(graph.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        let edges = ListThreadTaskEdgesQuery::new(graph.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        let ready_nodes = ListReadyThreadTaskNodesQuery::new(graph.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let snapshot = serde_json::json!({
            "graph": graph,
            "nodes": nodes,
            "edges": edges,
            "ready_node_ids": ready_nodes
                .iter()
                .map(|node| node.id.to_string())
                .collect::<Vec<_>>(),
        });

        self.task_graph_snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
}
