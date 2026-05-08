include!(concat!(env!("OUT_DIR"), "/system_skills_data.rs"));

#[derive(Debug, Clone)]
pub struct SystemSkillSpec {
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
}

impl SystemSkillSpec {
    pub fn mount_path(&self) -> String {
        format!("/skills/system/{}", self.slug)
    }
}

pub fn list_system_skills() -> Vec<SystemSkillSpec> {
    GENERATED_SYSTEM_SKILLS
        .iter()
        .map(|s| SystemSkillSpec {
            slug: s.slug.to_string(),
            name: s.name.to_string(),
            description: if s.description.is_empty() {
                None
            } else {
                Some(s.description.to_string())
            },
        })
        .collect()
}

pub fn read_system_skill_md(slug: &str) -> Option<&'static str> {
    GENERATED_SYSTEM_SKILLS
        .iter()
        .find(|s| s.slug == slug)
        .map(|s| s.skill_md)
}
