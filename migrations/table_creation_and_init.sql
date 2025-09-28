CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    phone_number TEXT UNIQUE,
    telegram_id TEXT UNIQUE,
    status TEXT CHECK (status IN ('pending_approval', 'active', 'suspended')) NOT NULL,
    platform TEXT CHECK (platform IN ('telegram', 'whatsapp', 'both')) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    approved_at TIMESTAMP WITH TIME ZONE
);

CREATE TABLE cost_rate_history (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    service_provider TEXT NOT NULL,
    cost_type TEXT NOT NULL,
    unit_cost DECIMAL(11,7) NOT NULL,
    unit_type TEXT NOT NULL,
    currency TEXT DEFAULT 'USD',
    effective_from TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);


CREATE TABLE query_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    query_text TEXT NOT NULL,
    query_type TEXT NOT NULL,
    response_type TEXT NOT NULL,
    error_message TEXT,
    total_cost DECIMAL(11,7) DEFAULT 0,
    processing_time_ms INTEGER,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    platform TEXT CHECK (platform IN ('telegram', 'whatsapp')) NOT NULL
);

CREATE TABLE cost_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID REFERENCES users(id),
    query_session_id UUID REFERENCES query_sessions(id),
    event_type TEXT NOT NULL,
    unit_cost DECIMAL(11,7) NOT NULL,
    unit_type TEXT NOT NULL,
    units_consumed INTEGER NOT NULL,
    cost_amount DECIMAL(11,7) NOT NULL,
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    platform TEXT CHECK (platform IN ('telegram', 'whatsapp')) NOT NULL
);

INSERT INTO cost_rate_history (service_provider, cost_type, unit_cost, unit_type, effective_from) VALUES
('twilio', 'whatsapp_incoming', 0.005, 'message', NOW()),
('twilio', 'whatsapp_marketing', 0.0107, 'message', NOW()),
('twilio', 'whatsapp_service', 0.005, 'message', NOW()),
('anthropic', 'input_token', 3.0, 'per_1m_tokens', NOW()),
('anthropic', 'output_token', 15.0, 'per_1m_tokens', NOW()),
('anthropic', '1h_cache_writes', 6.0, 'per_1m_tokens', NOW()),
('anthropic', '5m_cache_writes', 3.75, 'per_1m_tokens', NOW()),
('anthropic', 'cache_hit_refresh', 0.3, 'per_1m_tokens', NOW()),
('groq_gpt_oss_20b', 'input_token', 0.1, 'per_1m_tokens', NOW()),
('groq_gpt_oss_20b', 'output_token', 0.5, 'per_1m_tokens', NOW()),
('aws_textract', 'detect_text', 0.0015, 'per_page', NOW());
