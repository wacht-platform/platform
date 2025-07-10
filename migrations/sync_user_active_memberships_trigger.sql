CREATE OR REPLACE FUNCTION sync_user_active_memberships()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.user_id IS NOT NULL AND 
       (NEW.active_organization_membership_id IS DISTINCT FROM OLD.active_organization_membership_id OR
        NEW.active_workspace_membership_id IS DISTINCT FROM OLD.active_workspace_membership_id) THEN
        
        UPDATE users
        SET 
            active_organization_membership_id = NEW.active_organization_membership_id,
            active_workspace_membership_id = NEW.active_workspace_membership_id,
            updated_at = CURRENT_TIMESTAMP
        WHERE id = NEW.user_id;
    END IF;
    
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS sync_user_active_memberships_trigger ON signins;

CREATE TRIGGER sync_user_active_memberships_trigger
    AFTER UPDATE ON signins
    FOR EACH ROW
    EXECUTE FUNCTION sync_user_active_memberships();

CREATE INDEX IF NOT EXISTS idx_signins_user_id ON signins(user_id) 
    WHERE user_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_signins_active_memberships ON signins(
    user_id, 
    active_organization_membership_id, 
    active_workspace_membership_id
) WHERE user_id IS NOT NULL;