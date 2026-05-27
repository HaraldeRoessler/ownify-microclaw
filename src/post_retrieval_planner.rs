use serde_json::Value;

/// A parsed fragment from the memory context text.
#[derive(Debug, Clone)]
pub struct MemoryChunk {
    pub content: String,
    pub confidence: f32,
}

/// What kind of behavioral guidance has been inferred.
#[derive(Debug, Clone, PartialEq)]
pub enum InsightType {
    Avoidance,
    Recommendation,
    ToneShift,
    Constraint,
}

#[derive(Debug, Clone)]
pub struct PlannerInsight {
    pub relevance: f32,
    pub insight_type: InsightType,
    pub instruction: String,
    pub source_drawers: Vec<String>,
}

/// A keyword-triggered inference rule.
struct Rule {
    memory_keywords: &'static [&'static str],
    intent_keywords: &'static [&'static str],
    insight_type: InsightType,
    template: &'static str,
    relevance: f32,
}

/// Static rule table — ordered by priority (first match wins per category).
static RULES: &[Rule] = &[
    // --- Avoidance: deadline pressure ---
    Rule {
        memory_keywords: &["hate", "stress", "fertig machen", "deadline pressure", "burnout", "überfordert"],
        intent_keywords: &["pitch", "plan", "project", "prepare", "timeline", "schedule", "presentation", "proposal"],
        insight_type: InsightType::Avoidance,
        template: "Do NOT propose tight schedules, deadline pressure, or aggressive timelines. The user has expressed stress with time-sensitive commitments. Suggest relaxed, buffer-heavy approaches instead.",
        relevance: 0.92,
    },
    Rule {
        memory_keywords: &["hate", "stress", "fertig machen", "deadline pressure", "burnout"],
        intent_keywords: &[], // any task
        insight_type: InsightType::ToneShift,
        template: "Use calm, non-urgent language. Avoid phrases like 'ASAP', 'immediately', 'critical', or 'deadline'. Prefer supportive, low-pressure communication.",
        relevance: 0.85,
    },
    // --- Preference → Recommendation ---
    Rule {
        memory_keywords: &["prefer", "like", "love", "favorite", "most comfortable"],
        intent_keywords: &["pitch", "plan", "project", "prepare", "design", "build", "create", "setup"],
        insight_type: InsightType::Recommendation,
        template: "Check retrieved memories for established preferences (tools, workflows, communication styles). Default to the user's preferred approach unless the current task explicitly demands otherwise.",
        relevance: 0.80,
    },
    // --- Allergies / Dietary Restrictions ---
    Rule {
        memory_keywords: &["allergy", "can't eat", "vertrage nicht", "allergic", "unverträglich"],
        intent_keywords: &["food", "restaurant", "cooking", "essen", "lunch", "dinner", "rezept", "recipe"],
        insight_type: InsightType::Avoidance,
        template: "Exclude known allergens and dietary restrictions. Explicitly verify food safety before making recommendations.",
        relevance: 0.95,
    },
    Rule {
        memory_keywords: &["allergy", "can't eat", "vertrage nicht", "allergic"],
        intent_keywords: &["food", "restaurant", "cooking", "essen", "lunch", "dinner"],
        insight_type: InsightType::Constraint,
        template: "The user has dietary restrictions. Always ask about food preferences/allergies before suggesting restaurants or meals.",
        relevance: 0.90,
    },
    // --- Technical stack preferences ---
    Rule {
        memory_keywords: &["self-hosted", "open-source", "kubernetes", "docker"],
        intent_keywords: &["infrastructure", "deploy", "host", "setup", "install", "service", "tool"],
        insight_type: InsightType::Recommendation,
        template: "Prefer self-hosted, open-source solutions. Avoid proprietary SaaS. Default to Kubernetes/Docker deployment patterns.",
        relevance: 0.88,
    },
    // --- Communication style preferences ---
    Rule {
        memory_keywords: &["concise", "short", "direct", "no fluff", "get to the point"],
        intent_keywords: &[],
        insight_type: InsightType::ToneShift,
        template: "Keep responses concise and direct. Skip preamble and unnecessary explanations. Get straight to the actionable information.",
        relevance: 0.82,
    },
    Rule {
        memory_keywords: &["detail-oriented", "thorough", "comprehensive", "in depth", "explain fully"],
        intent_keywords: &[],
        insight_type: InsightType::ToneShift,
        template: "Provide thorough, detailed responses. Include rationale, trade-offs, and edge cases. The user values comprehensive understanding over brevity.",
        relevance: 0.82,
    },
];

pub struct PostRetrievalPlanner;

impl PostRetrievalPlanner {
    pub fn new() -> Self {
        Self
    }

    /// Parse the flat memory-context string into `MemoryChunk` entries.
    fn parse_chunks(&self, text: &str) -> Vec<MemoryChunk> {
        let mut chunks = Vec::new();

        // Split on common memory-entry delimiters: "- [score] " or "-\n"
        for block in text.split("\n- ") {
            let trimmed = block.trim();
            if trimmed.is_empty() || trimmed.len() < 5 {
                continue;
            }

            // Try to extract confidence from "[0.XX] " prefix
            let (content, confidence) = if let Some(rest) = trimmed.strip_prefix('[') {
                if let Some(bracket_end) = rest.find(']') {
                    let score_str = &rest[..bracket_end];
                    let conf = score_str.parse::<f32>().unwrap_or(0.5).clamp(0.0, 1.0);
                    (rest[bracket_end + 1..].trim().to_string(), conf)
                } else {
                    (trimmed.to_string(), 0.5)
                }
            } else {
                (trimmed.to_string(), 0.5)
            };

            chunks.push(MemoryChunk { content, confidence });
        }

        chunks
    }

    /// Main entry point — analyzes memory context and user intent to produce
    /// actionable planning instructions for the system prompt.
    pub fn plan(
        &self,
        mem_context: &str,
        user_input: &str,
    ) -> Vec<PlannerInsight> {
        let lower_mem = mem_context.to_lowercase();
        let lower_input = user_input.to_lowercase();
        let mut insights = Vec::new();

        for rule in RULES {
            // Test memory keywords
            let mem_match = if rule.memory_keywords.is_empty() {
                true
            } else {
                rule.memory_keywords
                    .iter()
                    .any(|kw| lower_mem.contains(*kw))
            };

            if !mem_match {
                continue;
            }

            // Test intent keywords (empty means match any intent)
            let intent_match = if rule.intent_keywords.is_empty() {
                true
            } else {
                rule.intent_keywords
                    .iter()
                    .any(|kw| lower_input.contains(*kw))
            };

            if !intent_match {
                continue;
            }

            // Find source chunks that triggered this rule
            let chunks = self.parse_chunks(mem_context);
            let source_drawers: Vec<String> = chunks
                .iter()
                .filter(|c| {
                    rule.memory_keywords
                        .iter()
                        .any(|kw| c.content.to_lowercase().contains(*kw))
                })
                .map(|c| c.content.chars().take(60).collect::<String>())
                .take(3)
                .collect();

            insights.push(PlannerInsight {
                relevance: rule.relevance,
                insight_type: rule.insight_type.clone(),
                instruction: rule.template.to_string(),
                source_drawers,
            });
        }

        insights.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
        insights
    }

    /// Format insights for insertion into the system prompt.
    pub fn format_for_prompt(&self, insights: &[PlannerInsight]) -> Option<String> {
        if insights.is_empty() {
            return None;
        }

        let mut buf = String::from(
            "[ACTIVE ADAPTATIONS]\n\
             The following rules are derived from your established preferences\n\
             and MUST be applied to the current response:\n\n",
        );

        for (i, insight) in insights.iter().enumerate() {
            let type_label = match insight.insight_type {
                InsightType::Avoidance => "AVOID",
                InsightType::Recommendation => "RECOMMEND",
                InsightType::ToneShift => "TONE",
                InsightType::Constraint => "CONSTRAINT",
            };
            buf.push_str(&format!(
                "{}. [{}, confidence: {:.2}] {}\n",
                i + 1,
                type_label,
                insight.relevance,
                insight.instruction,
            ));
        }

        buf.push('\n');
        Some(buf)
    }
}

impl Default for PostRetrievalPlanner {
    fn default() -> Self {
        Self::new()
    }
}

// --- LoopTrace payload helpers ---

pub fn planner_insights_payload(insights: &[PlannerInsight]) -> Value {
    let items: Vec<Value> = insights
        .iter()
        .map(|i| {
            serde_json::json!({
                "type": format!("{:?}", i.insight_type),
                "relevance": i.relevance,
                "instruction": i.instruction,
            })
        })
        .collect();
    serde_json::json!({ "insights_count": items.len(), "insights": items })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stress_pattern_triggers_avoidance() {
        let planner = PostRetrievalPlanner::new();
        let mem = "User hates deadlines and stress. Prefers relaxed planning.";
        let input = "Can you help me prepare a project pitch?";

        let insights = planner.plan(mem, input);
        assert!(!insights.is_empty());
        assert!(insights.iter().any(|i| i.insight_type == InsightType::Avoidance));
    }

    #[test]
    fn no_match_returns_empty() {
        let planner = PostRetrievalPlanner::new();
        let mem = "User lives in Berlin.";
        let input = "What is the weather today?";

        let insights = planner.plan(mem, input);
        assert!(insights.is_empty());
    }

    #[test]
    fn allergy_with_food_trigger() {
        let planner = PostRetrievalPlanner::new();
        let mem = "User has a severe peanut allergy.";
        let input = "Recommend a good restaurant for dinner.";

        let insights = planner.plan(mem, input);
        assert!(insights.iter().any(|i| i.insight_type == InsightType::Avoidance));
        assert!(insights.iter().any(|i| i.insight_type == InsightType::Constraint));
    }

    #[test]
    fn stress_without_planning_intent_tone_shift() {
        let planner = PostRetrievalPlanner::new();
        let mem = "User mentions burnout and stress frequently.";
        let input = "Tell me a joke.";

        let insights = planner.plan(mem, input);
        assert!(insights.iter().any(|i| i.insight_type == InsightType::ToneShift));
        assert!(!insights.iter().any(|i| i.insight_type == InsightType::Avoidance));
    }

    #[test]
    fn format_empty_insights_returns_none() {
        let planner = PostRetrievalPlanner::new();
        assert!(planner.format_for_prompt(&[]).is_none());
    }
}
