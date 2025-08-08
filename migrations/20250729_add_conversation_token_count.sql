-- Add token_count column to conversations table
-- This column will store the token count for each conversation message
-- to enable efficient token-based context window management

-- First, update the check constraint to include new message types
ALTER TABLE conversations DROP CONSTRAINT IF EXISTS conversations_message_type_check;

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
    'context_results'::text,
    'user_input_request'::text,
    'execution_summary'::text
]));

-- Add the token_count column
ALTER TABLE conversations 
ADD COLUMN token_count INTEGER DEFAULT 0;

-- Add index for efficient queries on token count
CREATE INDEX idx_conversations_context_token ON conversations(context_id, id DESC, token_count);

-- Update existing records to have a reasonable default
-- Using a rough estimate of 4 characters per token for existing messages
UPDATE conversations 
SET token_count = CASE 
    WHEN message_type = 'execution_summary' THEN 
        -- Execution summaries should have their token count from the JSON content
        COALESCE((content->>'token_count')::INTEGER, 250)
    ELSE 
        -- For other messages, estimate based on content length
        -- This is a rough estimate and should be recalculated with proper tokenizer
        GREATEST(10, LENGTH(content::text) / 4)
END
WHERE token_count = 0;

-- Add comment to document the column
COMMENT ON COLUMN conversations.token_count IS 'Number of tokens in this conversation message, calculated using tiktoken cl100k_base tokenizer';