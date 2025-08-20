-- Add context_group field to execution contexts for grouping and access control
ALTER TABLE agent_execution_contexts 
ADD COLUMN context_group TEXT;

-- Create index for efficient filtering by context_group
CREATE INDEX idx_agent_execution_contexts_context_group 
ON agent_execution_contexts(deployment_id, context_group) 
WHERE context_group IS NOT NULL;

-- Create index for context_group and status combination (common query pattern)
CREATE INDEX idx_agent_execution_contexts_context_group_status 
ON agent_execution_contexts(deployment_id, context_group, status) 
WHERE context_group IS NOT NULL;