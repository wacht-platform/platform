-- Add ownership column to projects table
ALTER TABLE projects
ADD COLUMN IF NOT EXISTS owner_id TEXT;

-- Create index for efficient querying
CREATE INDEX IF NOT EXISTS idx_projects_owner_id ON projects(owner_id);

-- Add comment for clarity
COMMENT ON COLUMN projects.owner_id IS 'ID of the user or organization that owns this project';

-- For existing projects, you might want to set a default owner
-- This is commented out by default - uncomment and modify as needed:
-- UPDATE projects SET owner_id = 'default-owner-id' WHERE owner_id IS NULL;