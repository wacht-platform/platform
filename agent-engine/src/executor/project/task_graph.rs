use super::core::AgentExecutor;
use common::error::AppError;
use models::thread_task_graph::status;
use models::ThreadTaskGraphSnapshot;
use queries::EnsureThreadTaskGraphSnapshotQuery;
use std::collections::{HashMap, HashSet};

impl AgentExecutor {
    pub(crate) fn invalidate_task_graph_snapshot(&mut self) {
        self.task_graph_snapshot = None;
    }

    pub(crate) fn render_task_graph_view(snapshot: &ThreadTaskGraphSnapshot) -> Option<String> {
        if snapshot.nodes.is_empty() {
            return None;
        }
        let ready_node_ids: HashSet<&str> = snapshot
            .ready_node_ids
            .iter()
            .map(String::as_str)
            .collect();

        let mut dependency_map: HashMap<String, Vec<String>> = HashMap::new();
        for edge in &snapshot.edges {
            dependency_map
                .entry(edge.to_node_id.to_string())
                .or_default()
                .push(edge.from_node_id.to_string());
        }

        let mut lines = vec![format!("Graph status: {}", snapshot.graph.status)];

        let ready_lines: Vec<String> = snapshot
            .nodes
            .iter()
            .filter(|node| ready_node_ids.contains(node.id.to_string().as_str()))
            .map(|node| format!("- {} {}", node.id, node.title))
            .collect();

        if ready_lines.is_empty() {
            lines.push("Ready nodes: none".to_string());
        } else {
            lines.push("Ready nodes:".to_string());
            lines.extend(ready_lines);
        }

        lines.push("All nodes:".to_string());

        for node in &snapshot.nodes {
            lines.push(format!("- {} {} [{}]", node.id, node.title, node.status));
            if let Some(depends_on) = dependency_map.get(&node.id.to_string()) {
                if !depends_on.is_empty() {
                    lines.push(format!("  depends_on: {}", depends_on.join(", ")));
                }
            }
        }

        Some(lines.join("\n"))
    }

    pub(crate) async fn fetch_task_graph_snapshot(
        &self,
    ) -> Result<ThreadTaskGraphSnapshot, AppError> {
        let mut query = EnsureThreadTaskGraphSnapshotQuery::new(
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
        );
        if let Some(board_item_id) = self.current_board_item_id() {
            query = query.with_board_item_id(Some(board_item_id));
        }
        query
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
    }

    pub(crate) async fn ensure_task_graph_snapshot(
        &mut self,
    ) -> Result<ThreadTaskGraphSnapshot, AppError> {
        if let Some(snapshot) = &self.task_graph_snapshot {
            if !matches!(
                snapshot.graph.status.as_str(),
                status::GRAPH_COMPLETED | status::GRAPH_FAILED | status::GRAPH_CANCELLED
            ) {
                return Ok(snapshot.clone());
            }
            self.task_graph_snapshot = None;
        }

        let snapshot = self.fetch_task_graph_snapshot().await?;
        self.task_graph_snapshot = Some(snapshot.clone());
        Ok(snapshot)
    }
}
