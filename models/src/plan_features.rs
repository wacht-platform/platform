use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanFeature {
    Webhooks,
    AiAgents,
    AiTools,
    AiKnowledgeBase,
    ApiKeys,
    Organizations,
    Workspaces,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanTier {
    #[serde(rename = "starter")]
    Starter,
    #[serde(rename = "growth")]
    Growth,
    #[serde(rename = "pro")]
    Pro,
}

impl PlanTier {
    pub fn from_product_id(product_id: &str) -> Option<Self> {
        match product_id {
            "pdt_6eSgfwefWhNkDH53uKxf8" => Some(Self::Starter),
            "pdt_QChhPSpP8MR2wFlAgOsIb" => Some(Self::Growth),
            "pdt_Zb4uCKxzPHusE1I1s5Evk" => Some(Self::Pro),
            _ => None,
        }
    }

    pub fn features(&self) -> HashSet<PlanFeature> {
        use PlanFeature::*;

        match self {
            PlanTier::Starter => {
                // Starter plan: CIAM suite only (limited usage)
                // No webhooks, no AI agents, no API keys
                HashSet::from([Organizations, Workspaces])
            }
            PlanTier::Growth => {
                // Growth plan: CIAM suite + Webhooks + API Keys
                HashSet::from([Webhooks, ApiKeys, Organizations, Workspaces])
            }
            PlanTier::Pro => {
                // Pro plan: Everything (CIAM suite + AI agents + webhooks + API keys)
                HashSet::from([
                    Webhooks,
                    AiAgents,
                    AiTools,
                    AiKnowledgeBase,
                    ApiKeys,
                    Organizations,
                    Workspaces,
                ])
            }
        }
    }

    pub fn has_feature(&self, feature: PlanFeature) -> bool {
        self.features().contains(&feature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starter_plan_features() {
        let plan = PlanTier::Starter;
        // Starter: CIAM only, no webhooks/AI/API keys
        assert!(!plan.has_feature(PlanFeature::Webhooks));
        assert!(!plan.has_feature(PlanFeature::AiAgents));
        assert!(!plan.has_feature(PlanFeature::ApiKeys));
        assert!(plan.has_feature(PlanFeature::Organizations));
        assert!(plan.has_feature(PlanFeature::Workspaces));
    }

    #[test]
    fn test_growth_plan_features() {
        let plan = PlanTier::Growth;
        // Growth: CIAM + Webhooks + API Keys
        assert!(plan.has_feature(PlanFeature::Webhooks));
        assert!(plan.has_feature(PlanFeature::ApiKeys));
        assert!(!plan.has_feature(PlanFeature::AiAgents));
        assert!(plan.has_feature(PlanFeature::Organizations));
    }

    #[test]
    fn test_pro_plan_features() {
        let plan = PlanTier::Pro;
        // Pro: Everything
        assert!(plan.has_feature(PlanFeature::Webhooks));
        assert!(plan.has_feature(PlanFeature::AiAgents));
        assert!(plan.has_feature(PlanFeature::ApiKeys));
        assert!(plan.has_feature(PlanFeature::Organizations));
    }
}
