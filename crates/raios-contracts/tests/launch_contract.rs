use raios_contracts::Command;

/// The exact JSON the native workspace sends. `rwk-core`'s daemon client test asserts the same
/// shape from the other side, so a rename on either end fails a test.
const WIRE_JSON: &str = r#"{"command_type":"ValidateAgentLaunch","payload":{"agent_name":"opencode","project_path":"/work/app","prompt":"inspect the build","idempotency_key":"v-1"}}"#;

fn command() -> Command {
    Command::ValidateAgentLaunch {
        agent_name: "opencode".into(),
        project_path: "/work/app".into(),
        prompt: Some("inspect the build".into()),
        idempotency_key: "v-1".into(),
    }
}

#[test]
fn validate_agent_launch_serializes_to_the_documented_wire_shape() {
    assert_eq!(serde_json::to_string(&command()).unwrap(), WIRE_JSON);
}

#[test]
fn validate_agent_launch_parses_from_the_documented_wire_shape() {
    assert_eq!(
        serde_json::from_str::<Command>(WIRE_JSON).unwrap(),
        command()
    );
}

#[test]
fn validate_agent_launch_exposes_its_idempotency_key() {
    assert_eq!(command().idempotency_key(), "v-1");
}

#[test]
fn a_missing_prompt_is_accepted_and_a_null_prompt_round_trips() {
    let without_prompt = r#"{"command_type":"ValidateAgentLaunch","payload":{"agent_name":"claude","project_path":"/work/app","prompt":null,"idempotency_key":"v-2"}}"#;
    let parsed = serde_json::from_str::<Command>(without_prompt).unwrap();
    assert_eq!(
        parsed,
        Command::ValidateAgentLaunch {
            agent_name: "claude".into(),
            project_path: "/work/app".into(),
            prompt: None,
            idempotency_key: "v-2".into(),
        }
    );
    assert_eq!(serde_json::to_string(&parsed).unwrap(), without_prompt);
}

#[test]
fn validate_agent_launch_is_a_different_command_from_launch_agent() {
    // The daemon opens a terminal window for `LaunchAgent`; the two must never be confusable
    // on the wire.
    let launch = Command::LaunchAgent {
        agent_name: "opencode".into(),
        project_path: "/work/app".into(),
        prompt: Some("inspect the build".into()),
        idempotency_key: "v-1".into(),
    };
    assert_ne!(command(), launch);
    assert!(serde_json::to_string(&launch)
        .unwrap()
        .contains(r#""command_type":"LaunchAgent""#));
}
