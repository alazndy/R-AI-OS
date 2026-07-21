//! Built-in discovery prompts for the first Product Factory intake cycle.
//!
//! Prompts are stable, versioned domain vocabulary. User answers are stored
//! separately by `question_key` and may be revised without changing a prompt.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FactoryIntakePrompt {
    pub key: &'static str,
    pub prompt: &'static str,
    pub required: bool,
}

pub const DISCOVERY_INTAKE_PROMPTS: [FactoryIntakePrompt; 5] = [
    FactoryIntakePrompt {
        key: "problem_statement",
        prompt: "What concrete problem must this product solve first?",
        required: true,
    },
    FactoryIntakePrompt {
        key: "target_user",
        prompt: "Who is the first user group, and what makes this problem urgent for them?",
        required: true,
    },
    FactoryIntakePrompt {
        key: "core_outcome",
        prompt: "What outcome should the first usable version deliver?",
        required: true,
    },
    FactoryIntakePrompt {
        key: "first_platform",
        prompt: "Which platform is first, and why is it the right initial constraint?",
        required: true,
    },
    FactoryIntakePrompt {
        key: "success_metric",
        prompt: "Which measurable signal proves the first release is useful?",
        required: true,
    },
];

pub const QUICK_INTAKE_PROMPTS: [FactoryIntakePrompt; 3] = [
    DISCOVERY_INTAKE_PROMPTS[0],
    DISCOVERY_INTAKE_PROMPTS[2],
    DISCOVERY_INTAKE_PROMPTS[4],
];

pub fn prompts_for_mode(mode: &str) -> &'static [FactoryIntakePrompt] {
    match mode {
        "quick" => &QUICK_INTAKE_PROMPTS,
        _ => &DISCOVERY_INTAKE_PROMPTS,
    }
}
