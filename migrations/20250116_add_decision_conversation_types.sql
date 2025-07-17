-- Migration: Add new conversation message types for decision-based execution flow
-- Date: 2025-01-16
-- Description: Adds system_decision and context_results message types to support the new decision-based agent architecture

-- Drop the existing check constraint
ALTER TABLE conversations 
DROP CONSTRAINT IF EXISTS conversations_message_type_check;

-- Add the updated check constraint with the new message types
ALTER TABLE conversations 
ADD CONSTRAINT conversations_message_type_check 
CHECK (message_type = ANY (ARRAY[
    'user_message'::text, 
    'agent_response'::text, 
    'assistant_acknowledgment'::text, 
    'assistant_ideation'::text, 
    'assistant_action_planning'::text, 
    'assistant_task_execution'::text, 
    'assistant_validation'::text,
    'system_decision'::text,
    'context_results'::text
]));

-- Add a composite index for better query performance with the new message types
CREATE INDEX IF NOT EXISTS idx_conversations_context_type_created 
ON conversations(context_id, message_type, created_at DESC);

-- Add comments to document the new message types
COMMENT ON COLUMN conversations.message_type IS 
'Type of conversation message. Valid values: 
- user_message: Message from the user
- agent_response: Final response from the agent
- assistant_acknowledgment: Initial acknowledgment of user request
- assistant_ideation: Strategic planning and reasoning
- assistant_action_planning: Planning specific actions to take
- assistant_task_execution: Executing planned tasks
- assistant_validation: Validating execution results
- system_decision: Decision-making steps in the orchestration flow (NEW)
- context_results: Search results from context gathering (NEW)';

-- Verify the migration
SELECT 
    constraint_name,
    check_clause
FROM information_schema.check_constraints
WHERE constraint_name = 'conversations_message_type_check';

-- Show current message type distribution
SELECT 
    message_type, 
    COUNT(*) as count
FROM conversations
GROUP BY message_type
ORDER BY message_type;