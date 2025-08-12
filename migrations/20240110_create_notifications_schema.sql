-- =====================================================
-- User-Facing Notifications System Schema (Minimal)
-- =====================================================
-- Simple in-app notifications
-- =====================================================

-- USER NOTIFICATIONS
-- =====================================================
-- Individual notification instances sent to users
CREATE TABLE notifications (
    id BIGSERIAL PRIMARY KEY,
    deployment_id BIGINT NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,

    -- Recipients
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    organization_id BIGINT REFERENCES organizations(id) ON DELETE CASCADE,
    workspace_id BIGINT REFERENCES workspaces(id) ON DELETE CASCADE,

    -- Content (plain text/markdown)
    title TEXT NOT NULL,
    body TEXT NOT NULL,

    -- Action (optional)
    action_url TEXT,
    action_label VARCHAR(100),

    -- Severity/Type for styling
    severity VARCHAR(20) NOT NULL DEFAULT 'info' CHECK (severity IN (
        'info',
        'success',
        'warning',
        'error'
    )),

    -- Status tracking
    is_read BOOLEAN DEFAULT FALSE,
    read_at TIMESTAMP WITH TIME ZONE,
    is_archived BOOLEAN DEFAULT FALSE,
    archived_at TIMESTAMP WITH TIME ZONE,

    -- Grouping (optional - for related notifications)
    group_id VARCHAR(255),
    group_count INTEGER DEFAULT 1,

    -- Deduplication (optional - prevent duplicates)
    dedupe_key VARCHAR(255),

    -- Source tracking
    source VARCHAR(100), -- 'system', 'workflow', 'user_action', etc.
    source_id VARCHAR(255), -- ID of source entity

    -- Metadata for additional context
    metadata JSONB,

    -- Timestamps
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE, -- Optional auto-expiry

    -- Constraints
    CHECK (title != ''),
    CHECK (body != '')
);

-- Indexes for performance
CREATE INDEX idx_notifications_user_unread ON notifications(user_id, is_read, created_at DESC) WHERE is_read = FALSE;
CREATE INDEX idx_notifications_user_all ON notifications(user_id, created_at DESC);
CREATE INDEX idx_notifications_user_archived ON notifications(user_id, is_archived, created_at DESC);
CREATE INDEX idx_notifications_organization ON notifications(organization_id, created_at DESC) WHERE organization_id IS NOT NULL;
CREATE INDEX idx_notifications_workspace ON notifications(workspace_id, created_at DESC) WHERE workspace_id IS NOT NULL;
CREATE INDEX idx_notifications_dedupe ON notifications(deployment_id, user_id, dedupe_key) WHERE dedupe_key IS NOT NULL;
CREATE INDEX idx_notifications_group ON notifications(group_id, created_at DESC) WHERE group_id IS NOT NULL;
CREATE INDEX idx_notifications_expires ON notifications(expires_at) WHERE expires_at IS NOT NULL;
