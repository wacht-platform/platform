use super::core::AgentExecutor;
use common::error::AppError;
use models::{
    ProjectTaskBoardAssignmentMetadata, ProjectTaskBoardAssignmentSpec, ProjectTaskBoardItem,
};
use queries::{ListAssignableAgentThreadsQuery, ListAssignmentsForThreadQuery};

fn assignment_role_thread_preference(
    assignment_role: &str,
    thread: &models::AgentThread,
) -> (bool, bool, bool) {
    let review_mismatch = matches!(
        assignment_role,
        models::project_task_board::assignment_role::REVIEWER
            | models::project_task_board::assignment_role::SPECIALIST_REVIEWER
    ) && thread.thread_purpose != models::agent_thread::purpose::REVIEW
        && thread.responsibility.as_deref() != Some("review");

    let service_mismatch = matches!(
        assignment_role,
        models::project_task_board::assignment_role::EXECUTOR
            | models::project_task_board::assignment_role::OBSERVER
    ) && thread.thread_purpose != models::agent_thread::purpose::EXECUTION;

    let approval_mismatch = assignment_role
        == models::project_task_board::assignment_role::APPROVER
        && thread.thread_purpose != models::agent_thread::purpose::REVIEW
        && thread.responsibility.as_deref() != Some("approval");

    (review_mismatch, service_mismatch, approval_mismatch)
}

#[derive(Debug, Clone)]
struct PlannedAssignmentEntry {
    thread_id: i64,
    assignment_role: String,
    status: String,
    instructions: Option<String>,
    metadata: serde_json::Value,
}

impl AgentExecutor {
    fn assignment_status_locks_chain(status: &str) -> bool {
        matches!(status, "claimed" | "in_progress")
    }

    fn assignment_status_is_mutable_plan(status: &str) -> bool {
        matches!(status, "pending" | "available" | "blocked")
    }

    fn assignment_status_is_active_plan(status: &str) -> bool {
        Self::assignment_status_locks_chain(status)
            || Self::assignment_status_is_mutable_plan(status)
    }

    fn normalize_planned_assignment_status(status: Option<&str>, index: usize) -> String {
        match (index, status) {
            (_, Some("blocked" | "completed" | "rejected")) => status.unwrap().to_string(),
            (0, _) => models::project_task_board::assignment_status::AVAILABLE.to_string(),
            _ => models::project_task_board::assignment_status::PENDING.to_string(),
        }
    }

    fn normalize_planned_assignment_chain(
        assignments: Vec<ProjectTaskBoardAssignmentSpec>,
    ) -> Vec<ProjectTaskBoardAssignmentSpec> {
        assignments
            .into_iter()
            .enumerate()
            .map(|(index, mut assignment)| {
                assignment.status = Some(Self::normalize_planned_assignment_status(
                    assignment.status.as_deref(),
                    index,
                ));
                assignment
            })
            .collect()
    }

    pub(crate) async fn ensure_project_task_board_assignments(
        &mut self,
        board_item: &ProjectTaskBoardItem,
        explicit_assignments: Option<Vec<ProjectTaskBoardAssignmentSpec>>,
    ) -> Result<bool, AppError> {
        let existing = queries::ListProjectTaskBoardItemAssignmentsQuery::new(board_item.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let assignments = explicit_assignments.unwrap_or_default();
        if assignments.is_empty() {
            return Ok(false);
        }

        let assignments = Self::normalize_planned_assignment_chain(assignments);

        let mut planned_entries = Vec::with_capacity(assignments.len());
        for (index, assignment) in assignments.iter().enumerate() {
            let status = assignment.status.clone().unwrap_or_else(|| {
                if index == 0 {
                    models::project_task_board::assignment_status::AVAILABLE.to_string()
                } else {
                    models::project_task_board::assignment_status::PENDING.to_string()
                }
            });
            let assignment_role = assignment.assignment_role.clone().unwrap_or_else(|| {
                models::project_task_board::assignment_role::EXECUTOR.to_string()
            });
            let resolved_thread_id = self
                .resolve_assignment_thread_id(board_item, assignment)
                .await?;
            let metadata = serde_json::to_value(ProjectTaskBoardAssignmentMetadata {
                requested_target: if assignment.target.thread_id.is_some() {
                    None
                } else {
                    Some(assignment.target.clone())
                },
            })?;

            planned_entries.push(PlannedAssignmentEntry {
                thread_id: resolved_thread_id,
                assignment_role,
                status,
                instructions: assignment.instructions.clone(),
                metadata,
            });
        }

        let mut consumed = std::collections::HashSet::<i64>::new();
        let mut changed = false;
        let deps = common::deps::from_app(&self.ctx.app_state).db().nats().id();

        for planned in &planned_entries {
            let matched = existing.iter().find(|a| {
                !consumed.contains(&a.id)
                    && Self::assignment_status_is_active_plan(&a.status)
                    && a.thread_id == planned.thread_id
                    && a.assignment_role == planned.assignment_role
                    && a.instructions == planned.instructions
            });

            if let Some(matched) = matched {
                consumed.insert(matched.id);
                if matched.status == models::project_task_board::assignment_status::AVAILABLE {
                    commands::UpdateProjectTaskBoardItemAssignmentStateCommand::new(
                        matched.id,
                        models::project_task_board::assignment_status::AVAILABLE.to_string(),
                    )
                    .with_note("Coordinator re-issued plan; rewake executor".to_string())
                    .execute_with_deps(&deps)
                    .await?;
                }
                continue;
            }

            commands::CreateProjectTaskBoardItemAssignmentCommand {
                id: self.ctx.app_state.sf.next_id()? as i64,
                board_item_id: board_item.id,
                thread_id: planned.thread_id,
                assignment_role: planned.assignment_role.clone(),
                status: planned.status.clone(),
                instructions: planned.instructions.clone(),
                metadata: planned.metadata.clone(),
            }
            .execute_with_deps(&deps)
            .await?;
            changed = true;
        }

        for existing_assignment in &existing {
            if consumed.contains(&existing_assignment.id) {
                continue;
            }
            if !Self::assignment_status_is_mutable_plan(&existing_assignment.status) {
                continue;
            }
            commands::UpdateProjectTaskBoardItemAssignmentStateCommand::new(
                existing_assignment.id,
                models::project_task_board::assignment_status::CANCELLED.to_string(),
            )
            .with_note("Assignment removed from the latest coordinator plan".to_string())
            .execute_with_deps(&deps)
            .await?;
            changed = true;
        }

        if changed {
            self.refresh_project_task_board_items().await?;
        }

        Ok(changed)
    }

    async fn resolve_assignment_thread_id(
        &mut self,
        board_item: &ProjectTaskBoardItem,
        assignment: &ProjectTaskBoardAssignmentSpec,
    ) -> Result<i64, AppError> {
        if let Some(thread_id) = assignment.target.thread_id {
            return Ok(thread_id.into_inner());
        }

        let current_thread = self.ctx.get_thread().await?;
        let reader = self
            .ctx
            .app_state
            .db_router
            .reader(common::ReadConsistency::Strong);
        let mut candidates = ListAssignableAgentThreadsQuery::new(
            self.ctx.agent.deployment_id,
            current_thread.project_id,
        )
        .execute_with_db(reader)
        .await?;

        if let Some(responsibility) = assignment.target.responsibility.as_ref() {
            candidates
                .retain(|thread| thread.responsibility.as_deref() == Some(responsibility.as_str()));
        }

        if !assignment.target.capability_tags.is_empty() {
            candidates.retain(|thread| {
                assignment.target.capability_tags.iter().all(|tag| {
                    thread
                        .capability_tags
                        .iter()
                        .any(|candidate| candidate == tag)
                })
            });
        }

        let assignment_role = assignment
            .assignment_role
            .as_deref()
            .unwrap_or(models::project_task_board::assignment_role::EXECUTOR);

        let mut ranked_candidates = Vec::with_capacity(candidates.len());
        for thread in candidates {
            let thread_reader = self
                .ctx
                .app_state
                .db_router
                .reader(common::ReadConsistency::Strong);
            let assignments = ListAssignmentsForThreadQuery::new(thread.id)
                .execute_with_db(thread_reader)
                .await?;
            let active_assignment_count = assignments
                .iter()
                .filter(|assignment| {
                    matches!(
                        assignment.status.as_str(),
                        "pending" | "available" | "claimed" | "in_progress" | "blocked"
                    )
                })
                .count();
            let is_busy = matches!(thread.status.as_str(), "running" | "waiting_for_input");
            let role_preferences = assignment_role_thread_preference(assignment_role, &thread);
            ranked_candidates.push((
                (
                    active_assignment_count,
                    is_busy,
                    role_preferences.0,
                    role_preferences.1,
                    role_preferences.2,
                    thread.thread_purpose == models::agent_thread::purpose::CONVERSATION,
                    !thread.reusable,
                    thread.updated_at,
                ),
                thread.id,
            ));
        }

        ranked_candidates.sort_by(|left, right| left.0.cmp(&right.0));

        ranked_candidates
            .into_iter()
            .next()
            .map(|(_, thread_id)| thread_id)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "No assignable thread matched the requested assignment target for board item {}",
                    board_item.id
                ))
            })
    }
}
