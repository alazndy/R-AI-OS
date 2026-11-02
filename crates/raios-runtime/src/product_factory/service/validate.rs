//! Input validation for Product Factory commands, plus the git-project inspection
//! and naming helpers the dispatcher uses.

use super::*;

pub(super) fn validate_command(command: &FactoryCommand) -> Result<(), Problem> {
    let valid = match command {
        FactoryCommand::CreateWorkspace {
            name,
            idempotency_key,
        } => valid_text(name, 120) && valid_idempotency_key(idempotency_key),
        FactoryCommand::CreateProductDraft {
            workspace_id,
            title,
            idempotency_key,
        } => {
            valid_identifier(workspace_id)
                && valid_text(title, 160)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::SetProductMode {
            product_id,
            idempotency_key,
            ..
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ScaffoldProject {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::AttachExistingProject {
            product_id,
            project_path,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_absolute_project_path(project_path)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::StartIntake {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::RecordIntakeAnswer {
            session_id,
            question_key,
            response,
            idempotency_key,
        } => {
            valid_identifier(session_id)
                && valid_identifier(question_key)
                && valid_text(response, 8_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreateCharterDraft {
            product_id,
            content,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(content, 32_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::GenerateCharterDraft {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::CreateRequirementDraft {
            product_id,
            stable_key,
            content,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_identifier(stable_key)
                && valid_text(content, 16_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::SubmitChangeRequest {
            product_id,
            summary,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(summary, 4_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::AssessChangeRequest {
            change_request_id,
            idempotency_key,
        } => valid_identifier(change_request_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ResolveImpactAssessment {
            assessment_id,
            idempotency_key,
            ..
        } => valid_identifier(assessment_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ApplyApprovedRequirementChange {
            assessment_id,
            requirement_id,
            content,
            idempotency_key,
        } => {
            valid_identifier(assessment_id)
                && valid_identifier(requirement_id)
                && valid_text(content, 16_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreatePlanDraft {
            product_id,
            title,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(title, 160)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::ApprovePlan {
            plan_id,
            idempotency_key,
        }
        | FactoryCommand::MaterializePlannedCycle {
            plan_id,
            idempotency_key,
        } => valid_identifier(plan_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::PauseCycle {
            cycle_id,
            idempotency_key,
        }
        | FactoryCommand::ResumeCycle {
            cycle_id,
            idempotency_key,
        }
        | FactoryCommand::CancelCycle {
            cycle_id,
            idempotency_key,
        } => valid_identifier(cycle_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::MaterializeStageTaskGraph {
            cycle_id,
            stage,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::ActivateApprovedStage {
            cycle_id,
            stage,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::RecordStageEvidence {
            cycle_id,
            stage,
            content_ref,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_text(content_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::LinkStageEvidenceToRequirement {
            evidence_id,
            requirement_id,
            idempotency_key,
        } => {
            valid_identifier(evidence_id)
                && valid_identifier(requirement_id)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CompleteStage {
            cycle_id,
            stage,
            idempotency_key,
        } => {
            valid_identifier(cycle_id)
                && is_factory_stage(stage)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::InspectReleaseReadiness {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::CreateQualityProfile {
            product_id,
            name,
            idempotency_key,
            ..
        } => {
            valid_identifier(product_id)
                && valid_text(name, 160)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::EnsureReactNativeClosedTestingQualityProfile {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::RecordQualityCheck {
            profile_id,
            evidence_ref,
            idempotency_key,
            ..
        } => {
            valid_identifier(profile_id)
                && valid_text(evidence_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreateSupportItem {
            product_id,
            source_kind,
            summary,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_identifier(source_kind)
                && valid_text(summary, 4_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::InspectSupportOverview {
            product_id,
            idempotency_key,
        } => valid_identifier(product_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::TriageSupportItem {
            support_item_id,
            idempotency_key,
        } => valid_identifier(support_item_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::ResolveSupportItem {
            support_item_id,
            resolution_ref,
            idempotency_key,
        } => {
            valid_identifier(support_item_id)
                && valid_text(resolution_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::LinkSupportToChangeRequest {
            support_item_id,
            change_request_id,
            idempotency_key,
        } => {
            valid_identifier(support_item_id)
                && valid_identifier(change_request_id)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::CreateReleaseDraft {
            product_id,
            build_ref,
            idempotency_key,
        } => {
            valid_identifier(product_id)
                && valid_text(build_ref, 2_000)
                && valid_idempotency_key(idempotency_key)
        }
        FactoryCommand::ApproveClosedTestingRelease {
            release_id,
            idempotency_key,
        } => valid_identifier(release_id) && valid_idempotency_key(idempotency_key),
        FactoryCommand::RequestImpactAssessment {
            change_request_id,
            idempotency_key,
        } => valid_identifier(change_request_id) && valid_idempotency_key(idempotency_key),
    };
    if valid {
        Ok(())
    } else {
        Err(Problem::invalid_input(
            "Factory command contains an empty, oversized, or invalid field",
        ))
    }
}

pub(super) fn valid_text(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_len
        && !value.contains('\0')
        && raios_core::security::looks_like_secret(value).is_none()
}

pub(super) fn valid_identifier(value: &str) -> bool {
    valid_text(value, 128)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

pub(super) fn valid_absolute_project_path(value: &str) -> bool {
    valid_text(value, 4_096) && Path::new(value).is_absolute()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExistingGitProject {
    pub(super) project_path: String,
    pub(super) remote: String,
    pub(super) revision: String,
}

/// Inspect an existing Git worktree without mutating it. The command accepts
/// only repository roots (not arbitrary subdirectories), captures a stable
/// origin and HEAD SHA, and never persists a credential-bearing HTTP remote.
pub(super) fn inspect_existing_git_project(
    project_path: &str,
) -> Result<ExistingGitProject, Problem> {
    let canonical_path = std::fs::canonicalize(project_path)
        .map_err(|_| Problem::invalid_input("Project path does not exist or is not accessible"))?;
    if !canonical_path.is_dir() || !canonical_path.join(".git").exists() {
        return Err(Problem::invalid_input(
            "Project path must be the root of an existing Git worktree",
        ));
    }

    let canonical = canonical_path.to_string_lossy().to_string();
    let inside_worktree = git_output(&canonical, &["rev-parse", "--is-inside-work-tree"])?;
    if inside_worktree != "true" {
        return Err(Problem::invalid_input(
            "Project path must be the root of an existing Git worktree",
        ));
    }
    let top_level = git_output(&canonical, &["rev-parse", "--show-toplevel"])?;
    // Git and std::fs can render the same Windows path differently (for
    // example `D:/repo` versus `\\\\?\\D:\\repo`). Compare canonical path
    // identities rather than their display strings.
    let canonical_top_level = std::fs::canonicalize(&top_level).map_err(|_| {
        Problem::invalid_input("Project path must be the root of an existing Git worktree")
    })?;
    if canonical_top_level != canonical_path {
        return Err(Problem::invalid_input(
            "Project path must be the root of an existing Git worktree",
        ));
    }
    let remote = git_output(&canonical, &["config", "--get", "remote.origin.url"])?;
    if remote.is_empty() || remote.len() > 2_000 || remote_has_embedded_http_credentials(&remote) {
        return Err(Problem::invalid_input(
            "Project origin remote is missing or contains embedded credentials",
        ));
    }
    let revision = git_output(&canonical, &["rev-parse", "HEAD"])?;
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Problem::invalid_input(
            "Project HEAD must resolve to a full Git commit SHA",
        ));
    }

    Ok(ExistingGitProject {
        project_path: canonical,
        remote,
        revision,
    })
}

pub(super) fn git_output(project_path: &str, args: &[&str]) -> Result<String, Problem> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .args(args)
        .output()
        .map_err(|_| Problem::invalid_input("Git is required to attach an existing project"))?;
    if !output.status.success() {
        return Err(Problem::invalid_input(
            "Project path is not a valid Git worktree with the required metadata",
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_string())
        .map_err(|_| Problem::invalid_input("Git metadata must be valid UTF-8"))
}

pub(super) fn remote_has_embedded_http_credentials(remote: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .filter_map(|scheme| remote.strip_prefix(scheme))
        .any(|rest| {
            rest.split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        })
}

pub(super) fn is_factory_stage(value: &str) -> bool {
    raios_core::db::FACTORY_LIFECYCLE_STAGES.contains(&value)
}

pub(super) fn valid_idempotency_key(value: &str) -> bool {
    valid_identifier(value) && value.len() >= 8
}

pub(super) fn factory_project_category(platform: &str) -> &'static str {
    let platform = platform.to_ascii_lowercase();
    if platform.contains("flutter")
        || platform.contains("react native")
        || platform.contains("android")
        || platform.contains("ios")
    {
        "mobile"
    } else if platform.contains("react") || platform.contains("next") || platform.contains("web") {
        "web"
    } else if platform.contains("esp") || platform.contains("iot") || platform.contains("embedded")
    {
        "embedded"
    } else if platform.contains("ai") || platform.contains("model") || platform.contains("data") {
        "ai"
    } else {
        "tools"
    }
}

pub(super) fn factory_project_slug(title: &str) -> String {
    let slug: String = title
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "factory-project".into()
    } else {
        slug.into()
    }
}
