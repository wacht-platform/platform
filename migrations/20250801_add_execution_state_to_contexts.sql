-- Add execution_state and status columns to agent_execution_contexts table
ALTER TABLE agent_execution_contexts
ADD COLUMN execution_state JSONB,
ADD COLUMN status TEXT NOT NULL DEFAULT 'idle';

-- Create index on status for faster queries
CREATE INDEX idx_agent_execution_contexts_status ON agent_execution_contexts(status);

-- Update existing contexts to have 'completed' status if they have a completed_at date
UPDATE agent_execution_contexts
SET status = 'completed'
WHERE completed_at IS NOT NULL;