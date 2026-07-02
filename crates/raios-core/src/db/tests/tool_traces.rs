use super::*;

#[test]
fn tool_trace_round_trip_search_and_forget() {
    let conn = in_memory();
    let id = tool_trace_insert(
        &conn,
        ToolTraceInsert {
            project: "gt-fit",
            agent: "codex_kaira",
            command: "./gradlew :androidApp:compileDebugKotlin",
            context: "Android debug build",
            outcome: "compiled after dependency fix",
            error_summary: "Unresolved reference HealthConnectClient",
            fix_summary: "Added missing dependency and corrected import path",
            tags_json: r#"["android","gradle"]"#,
            success: true,
            confidence: 0.9,
            related_task_id: None,
        },
    )
    .unwrap()
    .unwrap();

    let hits = tool_trace_search(
        &conn,
        ToolTraceQuery {
            text: "HealthConnect",
            project: Some("gt-fit"),
            preferred_project: Some("gt-fit"),
            success_only: true,
            tag: Some("android"),
            limit: 5,
        },
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, id);
    assert!(hits[0].success);

    assert!(tool_trace_forget(&conn, &id).unwrap());
    assert!(tool_trace_get(&conn, &id).unwrap().is_none());
}

#[test]
fn tool_trace_dedupes_exact_content() {
    let conn = in_memory();
    let trace = || ToolTraceInsert {
        project: "raios",
        agent: "claude_kaira",
        command: "cargo test",
        context: "db tests",
        outcome: "passed",
        error_summary: "",
        fix_summary: "added migration",
        tags_json: r#"["rust"]"#,
        success: true,
        confidence: 0.8,
        related_task_id: None,
    };

    assert!(tool_trace_insert(&conn, trace()).unwrap().is_some());
    assert!(tool_trace_insert(&conn, trace()).unwrap().is_none());
}

#[test]
fn tool_trace_secret_refusal_records_without_secret_content() {
    let conn = in_memory();
    let id =
        tool_trace_record_secret_refusal(&conn, "raios", "codex_kaira", "OpenAI API key").unwrap();
    let row = tool_trace_get(&conn, &id).unwrap().unwrap();
    assert!(row.redacted);
    assert_eq!(row.command, "[refused: secret-like content]");
    assert!(!row.outcome.contains("sk-"));
}

#[test]
fn tool_trace_search_excludes_redacted_rows() {
    let conn = in_memory();
    tool_trace_record_secret_refusal(&conn, "raios", "codex_kaira", "OpenAI API key").unwrap();

    let hits = tool_trace_search(
        &conn,
        ToolTraceQuery {
            text: "refused_secret",
            project: None,
            preferred_project: None,
            success_only: false,
            tag: None,
            limit: 10,
        },
    )
    .unwrap();

    assert!(hits.is_empty());
}

#[test]
fn tool_trace_search_prioritizes_preferred_project_without_filtering() {
    let conn = in_memory();
    tool_trace_insert(
        &conn,
        ToolTraceInsert {
            project: "other-project",
            agent: "claude_kaira",
            command: "cargo test",
            context: "shared failure",
            outcome: "passed",
            error_summary: "trace recall missed partial phrase",
            fix_summary: "less relevant fix",
            tags_json: r#"["trace"]"#,
            success: true,
            confidence: 0.9,
            related_task_id: None,
        },
    )
    .unwrap();
    tool_trace_insert(
        &conn,
        ToolTraceInsert {
            project: "R-AI-OS",
            agent: "codex_kaira",
            command: "cargo test",
            context: "shared failure",
            outcome: "passed",
            error_summary: "trace recall missed partial phrase",
            fix_summary: "preferred project fix",
            tags_json: r#"["trace"]"#,
            success: true,
            confidence: 0.1,
            related_task_id: None,
        },
    )
    .unwrap();

    let hits = tool_trace_search(
        &conn,
        ToolTraceQuery {
            text: "partial phrase",
            project: None,
            preferred_project: Some("R-AI-OS"),
            success_only: true,
            tag: None,
            limit: 10,
        },
    )
    .unwrap();

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].project, "R-AI-OS");
    assert_eq!(hits[0].fix_summary, "preferred project fix");
}
