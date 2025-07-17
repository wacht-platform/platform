-- Migration: Add new conversation message types for decision-based execution flow
-- Date: 2025-01-16
-- Description: Adds system_decision and context_results message types to support the new decision-based agent architecture

-- This is an alternative migration if the message_type column is TEXT instead of an enum

-- First, let's check the current column type
DO $$
DECLARE
    column_type TEXT;
BEGIN
    -- Get the data type of the message_type column
    SELECT data_type INTO column_type
    FROM information_schema.columns
    WHERE table_name = 'conversations' 
    AND column_name = 'message_type';
    
    -- If it's a text column, we just need to add a CHECK constraint
    IF column_type = 'text' OR column_type = 'character varying' THEN
        -- Drop existing constraint if it exists
        ALTER TABLE conversations 
        DROP CONSTRAINT IF EXISTS conversations_message_type_check;
        
        -- Add new constraint with all valid message types
        ALTER TABLE conversations 
        ADD CONSTRAINT conversations_message_type_check 
        CHECK (message_type IN (
            'user_message',
            'agent_response',
            'assistant_acknowledgment',
            'assistant_ideation',
            'assistant_action_planning',
            'assistant_task_execution',
            'assistant_validation',
            'system_decision',
            'context_results'
        ));
    END IF;
END $$;

-- Create indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_conversations_message_type 
ON conversations(message_type);

CREATE INDEX IF NOT EXISTS idx_conversations_created_at 
ON conversations(created_at DESC);

CREATE INDEX IF NOT EXISTS idx_conversations_context_type_created 
ON conversations(context_id, message_type, created_at DESC);

-- Add comments to document the new message types
COMMENT ON COLUMN conversations.message_type IS 
'Type of conversation message. Valid values: user_message, agent_response, assistant_acknowledgment, assistant_ideation, assistant_action_planning, assistant_task_execution, assistant_validation, system_decision (tracks decision-making steps), context_results (stores search results)';

-- Verify the migration by checking current message types
SELECT DISTINCT message_type, COUNT(*) as count
FROM conversations
GROUP BY message_type
ORDER BY message_type;