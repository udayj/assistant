-- Add conversation tracking for context preservation
-- Run this migration to add conversation support

-- Conversation tracking table
CREATE TABLE conversations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    last_activity_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Conversation message history for context
CREATE TABLE conversation_messages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    conversation_id UUID NOT NULL REFERENCES conversations(id),
    session_id UUID NOT NULL REFERENCES query_sessions(id),
    user_query TEXT NOT NULL,
    structured_response JSONB,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Link existing sessions to conversations
ALTER TABLE query_sessions ADD COLUMN conversation_id UUID REFERENCES conversations(id);

-- Indexes for efficient queries
CREATE INDEX idx_conversations_user_id ON conversations(user_id);
CREATE INDEX idx_conversation_messages_conversation_id ON conversation_messages(conversation_id);