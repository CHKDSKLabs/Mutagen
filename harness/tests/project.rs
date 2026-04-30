use mutagen_harness::adapter::HostKind;
use mutagen_harness::project::{
    ProjectAddFeatureOptions, ProjectApplyBlueprintOptions, ProjectCommandKind,
    ProjectCreateOptions, ProjectDashboardOptions, ProjectDoctorOptions,
    ProjectEnqueueFeatureOptions, ProjectExecuteFeatureOptions, ProjectFeatureFlowOptions,
    ProjectFeatureProgressOptions, ProjectFeatureStatusOptions, ProjectFeaturesOptions,
    ProjectInitOptions, ProjectInspectOptions, ProjectIntakeOptions, ProjectPlanFeatureOptions,
    ProjectPreviewCheckOptions, ProjectPreviewLifecycleOptions, ProjectPreviewPlanOptions,
    ProjectRepairOptions, ProjectRunCommandOptions, ProjectScaffoldOptions,
    ProjectSliceFeatureOptions, ProjectStatusOptions, ProjectVerifyGeneratedOptions, add_feature,
    apply_blueprint, create_project, dashboard_project, doctor_project, enqueue_feature,
    execute_feature, feature_flow, feature_progress, feature_status, init_project, inspect_project,
    list_blueprints, list_features, plan_feature, preview_check, preview_plan, preview_start,
    preview_status, preview_stop, project_intake, repair_project, run_project_command,
    scaffold_project, slice_feature, status_project, verify_generated_project,
};
use mutagen_harness::queue::SliceQueue;
use mutagen_harness::selected_slice::PrepareSelectedSliceResult;
use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn project_init_writes_capsule_and_builder_artifacts() {
    let workspace = TestWorkspace::new("project-init");

    let result = init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "crew-scheduler".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: Some("cloudflare".to_string()),
        force: false,
    })
    .expect("project init should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "initialized");
    assert_eq!(result.capsule.name, "crew-scheduler");
    assert_eq!(result.capsule.stack, "nextjs-postgres");
    assert_eq!(result.capsule.design_system, "shadcn");
    assert_eq!(result.capsule.deploy_target.as_deref(), Some("cloudflare"));

    for path in [
        ".mutagen/project.json",
        ".mutagen/design/brief.md",
        ".mutagen/design/tokens.json",
        ".mutagen/design/components.json",
        ".mutagen/state/decisions.jsonl",
        ".mutagen/state/build-log.jsonl",
        ".mutagen/state/deployments.jsonl",
        ".claude/workflow.json",
        "docs/PRD.md",
        "docs/ADR.md",
        "docs/DDD.md",
        "docs/ISC.md",
        "docs/DSD.md",
        "slices/queue.json",
        "project_state.md",
    ] {
        assert!(workspace.root.join(path).exists(), "{path} should exist");
    }

    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project inspect should succeed");

    assert!(inspect.ok);
    assert_eq!(inspect.status, "ready");
    assert!(inspect.missing_paths.is_empty());
}

#[test]
fn project_init_refuses_to_overwrite_existing_capsule_without_force() {
    let workspace = TestWorkspace::new("project-overwrite");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "first".to_string(),
        stack: "vite".to_string(),
        design_system: "css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("first project init should succeed");

    let error = init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "second".to_string(),
        stack: "nextjs".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect_err("second project init should require force");

    assert!(error.to_string().contains("project capsule already exists"));
}

#[test]
fn project_create_initializes_blueprints_and_scaffolds_project() {
    let workspace = TestWorkspace::new("project-create");

    let result = create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: Some("cloudflare".to_string()),
        force: false,
    })
    .expect("project create should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "created");
    assert_eq!(result.init.status, "initialized");
    assert_eq!(result.blueprint.status, "blueprint_applied");
    assert_eq!(result.scaffold.status, "scaffolded");
    assert_eq!(result.blueprint.capsule.stack, "vite-express-sqlite");
    assert_eq!(result.blueprint.capsule.commands.dev, "npm run dev");

    for path in [
        ".mutagen/project.json",
        "package.json",
        "src/App.jsx",
        "server/index.js",
        "vite.config.js",
    ] {
        assert!(workspace.root.join(path).exists(), "{path} should exist");
    }
}

#[test]
fn project_create_refuses_scaffold_collisions_before_initializing() {
    let workspace = TestWorkspace::new("project-create-collision");

    fs::write(workspace.root.join("package.json"), "{}\n").expect("package should write");

    let error = create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "collision-check".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect_err("project create should refuse scaffold collision");

    assert!(error.to_string().contains("create would overwrite"));
    assert!(!workspace.root.join(".mutagen/project.json").exists());
}

#[test]
fn project_inspect_reports_missing_artifacts() {
    let workspace = TestWorkspace::new("project-inspect");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "missing-check".to_string(),
        stack: "fastapi-react".to_string(),
        design_system: "tailwind".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");

    fs::remove_file(workspace.root.join("docs/PRD.md")).expect("test should remove PRD");

    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project inspect should succeed");

    assert!(!inspect.ok);
    assert_eq!(inspect.status, "incomplete");
    assert_eq!(inspect.missing_paths, vec!["docs/PRD.md"]);
}

#[test]
fn project_doctor_reports_rust_toolchain_for_bevy_stack() {
    let workspace = TestWorkspace::new("project-doctor-bevy");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "tiny-planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    let result = doctor_project(ProjectDoctorOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project doctor should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "ready");
    assert_eq!(result.stack, "rust-bevy");
    assert_eq!(
        result
            .checks
            .iter()
            .map(|check| check.executable.as_str())
            .collect::<Vec<_>>(),
        vec!["cargo", "rustc"]
    );
    assert!(result.checks.iter().all(|check| check.ok));
}

#[test]
fn project_status_summarizes_capsule_scaffold_doctor_preview_and_build_log() {
    let workspace = TestWorkspace::new("project-status");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Tiny Planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    replace_capsule_command(&workspace.root, "test", "true");
    run_project_command(ProjectRunCommandOptions {
        workspace_root: workspace.root.clone(),
        kind: ProjectCommandKind::Test,
        dry_run: false,
    })
    .expect("project command should run");

    let result = status_project(ProjectStatusOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project status should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "ready");
    assert_eq!(result.stack, "rust-bevy");
    assert!(result.capsule_ok);
    assert!(result.scaffold_ok);
    assert!(result.doctor_ok);
    assert_eq!(result.preview.status, "stopped");
    assert!(result.missing_paths.is_empty());
    assert!(result.missing_scaffold_paths.is_empty());
    assert_eq!(result.doctor.stack, "rust-bevy");

    let log_entry = result
        .last_build_log_entry
        .expect("status should include latest build log entry");
    assert_eq!(log_entry["command_kind"], "test");
    assert_eq!(log_entry["status"], "completed");
}

#[test]
fn project_status_reports_missing_scaffold_paths() {
    let workspace = TestWorkspace::new("project-status-missing-scaffold");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "Tiny Planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    let result = status_project(ProjectStatusOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project status should succeed");

    assert!(!result.ok);
    assert_eq!(result.status, "attention");
    assert!(result.capsule_ok);
    assert!(!result.scaffold_ok);
    assert!(
        result
            .missing_scaffold_paths
            .contains(&"Cargo.toml".to_string())
    );
    assert_eq!(result.last_build_log_entry, None);
}

#[test]
fn project_repair_scaffold_restores_missing_files_without_overwriting_existing_files() {
    let workspace = TestWorkspace::new("project-repair-scaffold");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Tiny Planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    fs::write(workspace.root.join("README.md"), "custom notes\n").expect("readme should write");
    fs::remove_file(workspace.root.join("Cargo.toml")).expect("manifest should remove");

    let result = repair_project(ProjectRepairOptions {
        workspace_root: workspace.root.clone(),
        scaffold: true,
        force: false,
    })
    .expect("project repair should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "repaired");
    assert_eq!(result.repaired_paths, vec!["Cargo.toml"]);
    assert!(result.overwritten_paths.is_empty());
    assert!(result.skipped_paths.contains(&"README.md".to_string()));
    assert_eq!(
        fs::read_to_string(workspace.root.join("README.md")).expect("readme should read"),
        "custom notes\n"
    );

    let status = status_project(ProjectStatusOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project status should succeed");

    assert!(status.scaffold_ok);
}

#[test]
fn project_repair_scaffold_force_overwrites_existing_files() {
    let workspace = TestWorkspace::new("project-repair-force");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Tiny Planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    fs::write(workspace.root.join("README.md"), "custom notes\n").expect("readme should write");

    let result = repair_project(ProjectRepairOptions {
        workspace_root: workspace.root.clone(),
        scaffold: true,
        force: true,
    })
    .expect("forced project repair should succeed");

    assert_eq!(result.status, "repaired_with_overwrites");
    assert!(result.repaired_paths.is_empty());
    assert!(result.overwritten_paths.contains(&"README.md".to_string()));
    assert!(result.skipped_paths.is_empty());
    assert!(
        fs::read_to_string(workspace.root.join("README.md"))
            .expect("readme should read")
            .contains("Generated by the Mutagen harness.")
    );
}

#[test]
fn project_repair_requires_selected_target() {
    let workspace = TestWorkspace::new("project-repair-no-target");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Tiny Planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let error = repair_project(ProjectRepairOptions {
        workspace_root: workspace.root.clone(),
        scaffold: false,
        force: false,
    })
    .expect_err("repair should require a target");

    assert!(error.to_string().contains("no repair target selected"));
}

#[test]
fn project_add_feature_records_intent_without_touching_scaffold() {
    let workspace = TestWorkspace::new("project-add-feature");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let app_before =
        fs::read_to_string(workspace.root.join("src/App.jsx")).expect("app should read");

    let result = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks should include optional due dates.".to_string(),
    })
    .expect("feature intent should be recorded");

    assert!(result.ok);
    assert_eq!(result.status, "feature_queued");
    assert!(result.feature.id.starts_with("feature-"));
    assert!(result.feature.id.ends_with("-add-due-dates"));
    assert_eq!(result.feature.title, "Add due dates");
    assert_eq!(result.feature.status, "queued");
    assert_eq!(result.feature.target_stack, "vite-express-sqlite");
    assert_eq!(
        fs::read_to_string(workspace.root.join("src/App.jsx")).expect("app should read"),
        app_before
    );

    let brief_path = workspace.root.join(&result.feature.brief_path);
    let brief = fs::read_to_string(brief_path).expect("feature brief should read");
    assert!(brief.contains("# Add due dates"));
    assert!(brief.contains("Tasks should include optional due dates."));

    let log = fs::read_to_string(workspace.root.join(".mutagen/state/features.jsonl"))
        .expect("features log should read");
    let entries = log.lines().collect::<Vec<_>>();
    assert_eq!(entries.len(), 1);
    let entry: Value = serde_json::from_str(entries[0]).expect("feature log should parse");
    assert_eq!(entry["id"], result.feature.id);
    assert_eq!(entry["title"], "Add due dates");
    assert_eq!(entry["target_stack"], "vite-express-sqlite");
}

#[test]
fn project_add_feature_requires_title() {
    let workspace = TestWorkspace::new("project-add-feature-title");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let error = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "  ".to_string(),
        description: String::new(),
    })
    .expect_err("feature title should be required");

    assert!(error.to_string().contains("feature title is required"));
}

#[test]
fn project_intake_updates_design_brief_without_queueing() {
    let workspace = TestWorkspace::new("project-intake-brief");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let result = project_intake(ProjectIntakeOptions {
        workspace_root: workspace.root.clone(),
        prompt: "Build a crew scheduling app for dispatchers. It should manage shifts, absences, and overtime."
            .to_string(),
        queue_feature: false,
        force: false,
    })
    .expect("project intake should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "brief_updated");
    assert_eq!(result.intake_mode, "brief_only");
    assert_eq!(result.title, "Build a crew scheduling app for dispatchers");
    assert!(result.feature_flow.is_none());
    assert!(result.queue_error.is_none());
    assert!(result.brief.excerpt.contains("crew scheduling app"));

    let brief = fs::read_to_string(workspace.root.join(".mutagen/design/brief.md"))
        .expect("design brief should read");
    assert!(brief.contains("## Current direction"));
    assert!(brief.contains("## Intake log"));
    assert!(brief.contains("Build a crew scheduling app for dispatchers."));

    let features_log = workspace.root.join(".mutagen/state/features.jsonl");
    assert!(!features_log.exists());
}

#[test]
fn project_intake_can_queue_natural_language_request() {
    let workspace = TestWorkspace::new("project-intake-queue");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let result = project_intake(ProjectIntakeOptions {
        workspace_root: workspace.root.clone(),
        prompt: "Build a crew scheduling app for dispatchers. It should manage shifts, absences, and overtime."
            .to_string(),
        queue_feature: true,
        force: false,
    })
    .expect("project intake should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "brief_updated_and_feature_flow_ready");
    assert_eq!(result.intake_mode, "brief_and_queue");
    assert_eq!(result.title, "Build a crew scheduling app for dispatchers");
    assert!(result.queue_error.is_none());
    assert!(result.feature_flow.is_some());

    let feature_flow = result.feature_flow.expect("feature flow should exist");
    assert_eq!(
        feature_flow.add_feature.feature.title,
        "Build a crew scheduling app for dispatchers"
    );
    assert_eq!(feature_flow.enqueue_feature.enqueued_slice_ids.len(), 3);

    let queue_raw = fs::read_to_string(workspace.root.join("slices/queue.json"))
        .expect("queue should be readable");
    let queue: SliceQueue = serde_json::from_str(&queue_raw).expect("valid queue");
    assert_eq!(queue.slices.len(), 3);
}

#[test]
fn project_features_lists_recorded_feature_intents() {
    let workspace = TestWorkspace::new("project-features-list");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks should include optional due dates.".to_string(),
    })
    .expect("first feature should be recorded");
    add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Filter completed tasks".to_string(),
        description: String::new(),
    })
    .expect("second feature should be recorded");

    let result = list_features(ProjectFeaturesOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("features should list");

    assert!(result.ok);
    assert_eq!(result.status, "ready");
    assert_eq!(result.features.len(), 2);
    assert_eq!(result.features[0].title, "Add due dates");
    assert_eq!(result.features[1].title, "Filter completed tasks");
    assert!(
        result
            .features
            .iter()
            .all(|feature| feature.status == "queued")
    );
}

#[test]
fn project_features_returns_empty_without_feature_log() {
    let workspace = TestWorkspace::new("project-features-empty");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let result = list_features(ProjectFeaturesOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("features should list");

    assert!(result.ok);
    assert_eq!(result.status, "empty");
    assert!(result.features.is_empty());
}

#[test]
fn project_plan_feature_writes_structured_plan_without_touching_scaffold() {
    let workspace = TestWorkspace::new("project-plan-feature");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks should include optional due dates.".to_string(),
    })
    .expect("feature should be recorded")
    .feature;
    let app_before =
        fs::read_to_string(workspace.root.join("src/App.jsx")).expect("app should read");

    let result = plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");

    assert!(result.ok);
    assert_eq!(result.status, "feature_planned");
    assert_eq!(result.feature.id, feature.id);
    assert_eq!(result.plan.feature_id, feature.id);
    assert_eq!(result.plan.status, "planned");
    assert_eq!(result.plan.target_stack, "vite-express-sqlite");
    assert!(
        result
            .plan
            .target_paths
            .contains(&"src/App.jsx".to_string())
    );
    assert!(
        result
            .plan
            .verification_commands
            .contains(&"npm run build".to_string())
    );
    assert_eq!(
        fs::read_to_string(workspace.root.join("src/App.jsx")).expect("app should read"),
        app_before
    );

    let plan_path = workspace.root.join(&result.plan.plan_path);
    let raw_plan = fs::read_to_string(plan_path).expect("plan should read");
    let plan: Value = serde_json::from_str(&raw_plan).expect("plan should parse");
    assert_eq!(plan["feature_id"], feature.id);
    assert_eq!(plan["title"], "Add due dates");
}

#[test]
fn project_plan_feature_refuses_to_overwrite_without_force() {
    let workspace = TestWorkspace::new("project-plan-feature-overwrite");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");

    let error = plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect_err("plan should require force before overwrite");

    assert!(error.to_string().contains("feature plan already exists"));

    let forced = plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id,
        force: true,
    })
    .expect("forced plan should succeed");

    assert_eq!(forced.status, "feature_planned");
}

#[test]
fn project_plan_feature_reports_unknown_feature_id() {
    let workspace = TestWorkspace::new("project-plan-feature-missing");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let error = plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: "feature-nope".to_string(),
        force: false,
    })
    .expect_err("unknown feature should fail");

    assert!(
        error
            .to_string()
            .contains("feature `feature-nope` was not found")
    );
}

#[test]
fn project_feature_status_reports_needs_plan_before_planning() {
    let workspace = TestWorkspace::new("project-feature-status-needs-plan");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;

    let result = feature_status(ProjectFeatureStatusOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
    })
    .expect("feature status should succeed");

    assert!(!result.ok);
    assert_eq!(result.status, "needs_plan");
    assert_eq!(result.feature.id, feature.id);
    assert!(result.brief_exists);
    assert!(!result.plan_exists);
    assert!(result.plan.is_none());
}

#[test]
fn project_feature_status_reports_ready_after_planning() {
    let workspace = TestWorkspace::new("project-feature-status-ready");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");

    let result = feature_status(ProjectFeatureStatusOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
    })
    .expect("feature status should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "ready");
    assert_eq!(result.feature.id, feature.id);
    assert!(result.brief_exists);
    assert!(result.plan_exists);
    assert_eq!(
        result.plan.expect("plan should be present").feature_id,
        feature.id
    );
}

#[test]
fn project_feature_status_reports_unknown_feature_id() {
    let workspace = TestWorkspace::new("project-feature-status-missing");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let error = feature_status(ProjectFeatureStatusOptions {
        workspace_root: workspace.root.clone(),
        feature_id: "feature-nope".to_string(),
    })
    .expect_err("unknown feature should fail");

    assert!(
        error
            .to_string()
            .contains("feature `feature-nope` was not found")
    );
}

#[test]
fn project_slice_feature_writes_feature_slice_manifest() {
    let workspace = TestWorkspace::new("project-slice-feature");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");

    let result = slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should slice");

    assert!(result.ok);
    assert_eq!(result.status, "feature_sliced");
    assert_eq!(result.feature.id, feature.id);
    assert_eq!(result.manifest.feature_id, feature.id);
    assert_eq!(result.manifest.status, "sliced");
    assert_eq!(result.manifest.slices.len(), 3);
    assert_eq!(result.manifest.slices[0].id, "slice-001-understand");
    assert!(
        workspace
            .root
            .join(format!(".mutagen/features/{}/slices.json", feature.id))
            .exists()
    );
}

#[test]
fn project_slice_feature_requires_plan() {
    let workspace = TestWorkspace::new("project-slice-feature-needs-plan");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;

    let error = slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id,
        force: false,
    })
    .expect_err("feature without plan should fail");

    assert!(error.to_string().contains("run plan-feature first"));
}

#[test]
fn project_slice_feature_requires_force_to_overwrite() {
    let workspace = TestWorkspace::new("project-slice-feature-force");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");
    slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should slice");

    let error = slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id,
        force: false,
    })
    .expect_err("second slice should require force");

    assert!(error.to_string().contains("pass --force to overwrite"));
}

#[test]
fn project_enqueue_feature_imports_slices_into_queue() {
    let workspace = TestWorkspace::new("project-enqueue-feature");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");
    slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should slice");

    let result = enqueue_feature(ProjectEnqueueFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should enqueue");

    assert!(result.ok);
    assert_eq!(result.status, "feature_enqueued");
    assert_eq!(result.enqueued_slice_ids.len(), 3);
    assert!(result.replaced_slice_ids.is_empty());
    assert_eq!(result.queue_slice_count, 3);

    let queue_raw = fs::read_to_string(workspace.root.join("slices/queue.json"))
        .expect("queue should be readable");
    let queue: SliceQueue = serde_json::from_str(&queue_raw).expect("valid queue");
    assert_eq!(queue.slices.len(), 3);
    assert_eq!(
        queue.slices[0].id,
        format!("{}-slice-001-understand", feature.id)
    );
    assert!(queue.slices[0].depends_on.is_empty());
    assert_eq!(queue.slices[1].depends_on, vec![queue.slices[0].id.clone()]);
    assert_eq!(queue.slices[2].depends_on, vec![queue.slices[1].id.clone()]);
    assert_eq!(queue.slices[0].traces_to.prd, vec![feature.id.clone()]);

    let prd = fs::read_to_string(workspace.root.join("docs/PRD.md")).expect("PRD should read");
    assert!(prd.contains(&feature.id));
    assert!(prd.contains("Tasks need optional due dates."));
}

#[test]
fn project_enqueue_feature_requires_slices() {
    let workspace = TestWorkspace::new("project-enqueue-feature-needs-slices");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");

    let error = enqueue_feature(ProjectEnqueueFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id,
        force: false,
    })
    .expect_err("feature without slices should fail");

    assert!(error.to_string().contains("run slice-feature first"));
}

#[test]
fn project_enqueue_feature_requires_force_to_replace_existing_queue_slices() {
    let workspace = TestWorkspace::new("project-enqueue-feature-force");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;
    plan_feature(ProjectPlanFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should plan");
    slice_feature(ProjectSliceFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should slice");
    enqueue_feature(ProjectEnqueueFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect("feature should enqueue");

    let error = enqueue_feature(ProjectEnqueueFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id.clone(),
        force: false,
    })
    .expect_err("second enqueue should require force");
    assert!(error.to_string().contains("pass --force to replace"));

    let result = enqueue_feature(ProjectEnqueueFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id,
        force: true,
    })
    .expect("force should replace existing feature slices");

    assert_eq!(result.status, "feature_reenqueued");
    assert_eq!(result.enqueued_slice_ids.len(), 3);
    assert_eq!(result.replaced_slice_ids.len(), 3);
    assert_eq!(result.queue_slice_count, 3);
}

#[test]
fn project_feature_flow_prepares_feature_for_execution() {
    let workspace = TestWorkspace::new("project-feature-flow");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let result = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
        force: false,
    })
    .expect("feature flow should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "feature_flow_ready");
    assert_eq!(result.feature_id, result.add_feature.feature.id);
    assert_eq!(result.plan_feature.plan.feature_id, result.feature_id);
    assert_eq!(result.slice_feature.manifest.feature_id, result.feature_id);
    assert_eq!(result.enqueue_feature.enqueued_slice_ids.len(), 3);
    assert_eq!(result.enqueue_feature.queue_slice_count, 3);

    assert!(
        workspace
            .root
            .join(format!(".mutagen/features/{}/brief.md", result.feature_id))
            .exists()
    );
    assert!(
        workspace
            .root
            .join(format!(".mutagen/features/{}/plan.json", result.feature_id))
            .exists()
    );
    assert!(
        workspace
            .root
            .join(format!(
                ".mutagen/features/{}/slices.json",
                result.feature_id
            ))
            .exists()
    );

    let queue_raw = fs::read_to_string(workspace.root.join("slices/queue.json"))
        .expect("queue should be readable");
    let queue: SliceQueue = serde_json::from_str(&queue_raw).expect("valid queue");
    assert_eq!(queue.slices.len(), 3);
}

#[test]
fn project_feature_flow_requires_title() {
    let workspace = TestWorkspace::new("project-feature-flow-title");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");

    let error = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: " ".to_string(),
        description: String::new(),
        force: false,
    })
    .expect_err("blank title should fail");

    assert!(error.to_string().contains("feature title is required"));
}

#[test]
fn project_execute_feature_prepares_next_feature_slice() {
    let workspace = TestWorkspace::new("project-execute-feature");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let flow = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
        force: false,
    })
    .expect("feature flow should succeed");

    let result = execute_feature(ProjectExecuteFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: flow.feature_id.clone(),
        host: HostKind::Stub,
        dry_run: false,
    })
    .expect("feature execution should prepare a slice");

    assert!(result.ok);
    assert_eq!(result.status, "feature_slice_ready");
    assert_eq!(
        result.selected_slice_id.as_deref(),
        Some(flow.enqueue_feature.enqueued_slice_ids[0].as_str())
    );
    match result.prepare.expect("prepare result should be present") {
        PrepareSelectedSliceResult::Ready { prepared } => {
            assert_eq!(
                prepared.slice_id,
                flow.enqueue_feature.enqueued_slice_ids[0]
            );
            assert!(prepared.claimed);
        }
        other => panic!("expected ready prepare result, got {other:?}"),
    }

    let queue_raw = fs::read_to_string(workspace.root.join("slices/queue.json"))
        .expect("queue should be readable");
    let queue: Value = serde_json::from_str(&queue_raw).expect("queue should parse");
    assert_eq!(queue["slices"][0]["status"], "in_progress");
    assert!(
        workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists()
    );
}

#[test]
fn project_execute_feature_dry_run_does_not_claim_slice() {
    let workspace = TestWorkspace::new("project-execute-feature-dry-run");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let flow = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
        force: false,
    })
    .expect("feature flow should succeed");

    let result = execute_feature(ProjectExecuteFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: flow.feature_id,
        host: HostKind::Stub,
        dry_run: true,
    })
    .expect("feature execution dry run should prepare a slice");

    assert!(result.ok);
    match result.prepare.expect("prepare result should be present") {
        PrepareSelectedSliceResult::Ready { prepared } => {
            assert!(!prepared.claimed);
        }
        other => panic!("expected ready prepare result, got {other:?}"),
    }

    let queue_raw = fs::read_to_string(workspace.root.join("slices/queue.json"))
        .expect("queue should be readable");
    let queue: Value = serde_json::from_str(&queue_raw).expect("queue should parse");
    assert_eq!(queue["slices"][0]["status"], "pending");
    assert!(
        !workspace
            .root
            .join(".mutagen/state/active-slice.json")
            .exists()
    );
}

#[test]
fn project_feature_progress_reports_queued_slices() {
    let workspace = TestWorkspace::new("project-feature-progress-queued");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let flow = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
        force: false,
    })
    .expect("feature flow should succeed");

    let result = feature_progress(ProjectFeatureProgressOptions {
        workspace_root: workspace.root.clone(),
        feature_id: flow.feature_id,
    })
    .expect("feature progress should load");

    assert!(result.ok);
    assert_eq!(result.status, "queued");
    assert_eq!(result.total, 3);
    assert_eq!(result.counts.pending, 3);
    assert_eq!(result.counts.in_progress, 0);
    assert_eq!(result.counts.completed, 0);
    assert!(result.active_slice.is_none());
    assert_eq!(result.slices.len(), 3);
}

#[test]
fn project_feature_progress_reports_active_slice() {
    let workspace = TestWorkspace::new("project-feature-progress-active");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let flow = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
        force: false,
    })
    .expect("feature flow should succeed");
    execute_feature(ProjectExecuteFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: flow.feature_id.clone(),
        host: HostKind::Stub,
        dry_run: false,
    })
    .expect("feature execution should prepare a slice");

    let result = feature_progress(ProjectFeatureProgressOptions {
        workspace_root: workspace.root.clone(),
        feature_id: flow.feature_id,
    })
    .expect("feature progress should load");

    assert!(result.ok);
    assert_eq!(result.status, "in_progress");
    assert_eq!(result.total, 3);
    assert_eq!(result.counts.pending, 2);
    assert_eq!(result.counts.in_progress, 1);
    let active_slice = result.active_slice.expect("active slice should be shown");
    assert_eq!(active_slice.id, result.slices[0].id);
    assert_eq!(active_slice.host, HostKind::Stub);
}

#[test]
fn project_feature_progress_reports_not_enqueued() {
    let workspace = TestWorkspace::new("project-feature-progress-not-enqueued");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let feature = add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded")
    .feature;

    let result = feature_progress(ProjectFeatureProgressOptions {
        workspace_root: workspace.root.clone(),
        feature_id: feature.id,
    })
    .expect("feature progress should load");

    assert!(!result.ok);
    assert_eq!(result.status, "not_enqueued");
    assert_eq!(result.total, 0);
    assert!(result.slices.is_empty());
}

#[test]
fn project_dashboard_summarizes_project_and_backlog() {
    let workspace = TestWorkspace::new("project-dashboard");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    project_intake(ProjectIntakeOptions {
        workspace_root: workspace.root.clone(),
        prompt: "Build a crew scheduling app for dispatchers. It should manage shifts, absences, and overtime."
            .to_string(),
        queue_feature: false,
        force: false,
    })
    .expect("project intake should succeed");
    add_feature(ProjectAddFeatureOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: String::new(),
    })
    .expect("feature should be recorded");

    let result = dashboard_project(ProjectDashboardOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("dashboard should load");

    assert!(result.ok);
    assert_eq!(result.status, result.project.status);
    assert_eq!(result.project.stack, "vite-express-sqlite");
    assert_eq!(result.feature_backlog.total, 1);
    assert_eq!(result.feature_backlog.queued, 1);
    assert_eq!(result.feature_backlog.planned, 0);
    assert_eq!(result.feature_backlog.ready, 0);
    assert_eq!(result.feature_backlog.in_queue, 0);
    assert!(result.project_brief.exists);
    assert!(
        result
            .project_brief
            .excerpt
            .contains("crew scheduling app for dispatchers")
    );
    assert!(result.active_feature.is_none());
}

#[test]
fn project_dashboard_includes_active_feature_progress() {
    let workspace = TestWorkspace::new("project-dashboard-active");

    create_project(ProjectCreateOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project create should succeed");
    let flow = feature_flow(ProjectFeatureFlowOptions {
        workspace_root: workspace.root.clone(),
        title: "Add due dates".to_string(),
        description: "Tasks need optional due dates.".to_string(),
        force: false,
    })
    .expect("feature flow should succeed");
    execute_feature(ProjectExecuteFeatureOptions {
        workspace_root: workspace.root.clone(),
        feature_id: flow.feature_id,
        host: HostKind::Stub,
        dry_run: false,
    })
    .expect("feature execution should prepare a slice");

    let result = dashboard_project(ProjectDashboardOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("dashboard should load");

    assert!(result.ok);
    assert_eq!(result.feature_backlog.total, 1);
    assert_eq!(result.feature_backlog.in_queue, 1);
    assert_eq!(
        result
            .active_feature
            .expect("active feature should be present")
            .status,
        "in_progress"
    );
}

#[test]
fn project_blueprint_catalog_lists_supported_stacks() {
    let result = list_blueprints();

    assert!(result.ok);
    assert!(
        result
            .blueprints
            .iter()
            .any(|blueprint| blueprint.stack == "nextjs-postgres")
    );
    assert!(
        result
            .blueprints
            .iter()
            .any(|blueprint| blueprint.stack == "vite-express-sqlite")
    );
    assert!(
        result
            .blueprints
            .iter()
            .any(|blueprint| blueprint.stack == "fastapi-react")
    );
    assert!(
        result
            .blueprints
            .iter()
            .any(|blueprint| blueprint.stack == "aspnet-blazor")
    );
    assert!(
        result
            .blueprints
            .iter()
            .any(|blueprint| blueprint.stack == "cloudflare-worker")
    );
    assert!(
        result
            .blueprints
            .iter()
            .any(|blueprint| blueprint.stack == "rust-bevy")
    );
}

#[test]
fn project_apply_blueprint_updates_capsule_commands() {
    let workspace = TestWorkspace::new("project-blueprint");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "crew-scheduler".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");

    let result = apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "blueprint_applied");
    assert_eq!(result.blueprint.stack, "nextjs-postgres");
    assert_eq!(result.capsule.commands.setup, "npm install");
    assert_eq!(result.capsule.commands.dev, "npm run dev");
    assert_eq!(result.capsule.preview.url, "http://localhost:3000");

    let inspect = inspect_project(ProjectInspectOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project inspect should succeed");

    assert_eq!(inspect.capsule.commands.build, "npm run build");
}

#[test]
fn project_apply_blueprint_can_switch_stack() {
    let workspace = TestWorkspace::new("project-blueprint-switch");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "ops-console".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");

    let result = apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: Some("aspnet-blazor".to_string()),
    })
    .expect("blueprint apply should switch stack");

    assert_eq!(result.capsule.stack, "aspnet-blazor");
    assert_eq!(result.capsule.commands.setup, "dotnet restore");
    assert_eq!(result.capsule.commands.test, "dotnet test");
}

#[test]
fn project_scaffold_materializes_vite_express_sqlite_project() {
    let workspace = TestWorkspace::new("project-scaffold-vite");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "Crew Scheduler".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    let result = scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace.root.clone(),
        force: false,
    })
    .expect("project scaffold should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "scaffolded");
    assert_eq!(result.stack, "vite-express-sqlite");
    assert!(result.overwritten_paths.is_empty());

    for path in [
        "package.json",
        "index.html",
        "src/App.jsx",
        "src/main.jsx",
        "src/styles.css",
        "server/db.js",
        "server/index.js",
        "server/db.test.js",
        "scripts/dev.mjs",
        "vite.config.js",
        "README.md",
        "data/.gitkeep",
    ] {
        assert!(workspace.root.join(path).exists(), "{path} should exist");
        assert!(
            result.created_paths.contains(&path.to_string()),
            "{path} should be reported as created"
        );
    }

    let package_raw =
        fs::read_to_string(workspace.root.join("package.json")).expect("package should read");
    let package: Value = serde_json::from_str(&package_raw).expect("package should parse");

    assert_eq!(package["name"], "crew-scheduler");
    assert_eq!(package["scripts"]["dev"], "node scripts/dev.mjs");
    assert_eq!(package["scripts"]["test"], "node --test");
}

#[test]
fn project_scaffold_refuses_overwrite_without_force() {
    let workspace = TestWorkspace::new("project-scaffold-overwrite");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "overwrite-check".to_string(),
        stack: "vite-express-sqlite".to_string(),
        design_system: "plain-css".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    fs::write(workspace.root.join("package.json"), "{}\n").expect("package should write");

    let error = scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace.root.clone(),
        force: false,
    })
    .expect_err("project scaffold should refuse overwrite");

    assert!(error.to_string().contains("scaffold would overwrite"));

    let forced = scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace.root.clone(),
        force: true,
    })
    .expect("forced project scaffold should succeed");

    assert_eq!(forced.status, "scaffolded_with_overwrites");
    assert!(
        forced
            .overwritten_paths
            .contains(&"package.json".to_string())
    );
}

#[test]
fn project_scaffold_materializes_rust_bevy_project() {
    let workspace = TestWorkspace::new("project-scaffold-bevy");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "Tiny Planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    let result = scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace.root.clone(),
        force: false,
    })
    .expect("project scaffold should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "scaffolded");
    assert_eq!(result.stack, "rust-bevy");

    for path in ["Cargo.toml", "src/main.rs", "README.md", ".gitignore"] {
        assert!(workspace.root.join(path).exists(), "{path} should exist");
        assert!(
            result.created_paths.contains(&path.to_string()),
            "{path} should be reported as created"
        );
    }

    let manifest =
        fs::read_to_string(workspace.root.join("Cargo.toml")).expect("bevy manifest should read");
    let main =
        fs::read_to_string(workspace.root.join("src/main.rs")).expect("bevy main should read");

    assert!(manifest.contains("name = \"tiny-planet\""));
    assert!(manifest.contains("bevy = \"0.18.1\""));
    assert!(main.contains("title: \"Tiny Planet\".to_string()"));
    assert!(main.contains("commands.spawn(Camera2d);"));
}

#[test]
fn project_scaffold_materializes_all_catalog_web_and_service_stacks() {
    let cases = vec![
        (
            "nextjs-postgres",
            vec![
                "package.json",
                "app/layout.jsx",
                "app/page.jsx",
                "app/api/items/route.js",
                "app/globals.css",
                "lib/items.js",
                "tests/items.test.js",
                ".env.example",
                "README.md",
            ],
            vec![
                ("package.json", "\"dev\": \"next dev\""),
                ("lib/items.js", "postgresConnectionString"),
            ],
        ),
        (
            "fastapi-react",
            vec![
                "package.json",
                "requirements.txt",
                "api/__init__.py",
                "api/main.py",
                "tests/test_api.py",
                "index.html",
                "src/main.jsx",
                "src/App.jsx",
                "src/styles.css",
                "tests/frontend.test.js",
                "scripts/dev.mjs",
                "vite.config.js",
                "README.md",
            ],
            vec![
                ("package.json", "\"dev\": \"node scripts/dev.mjs\""),
                ("api/main.py", "FastAPI"),
            ],
        ),
        (
            "aspnet-blazor",
            vec![
                "MutagenGeneratedApp.csproj",
                "Program.cs",
                "Components/_Imports.razor",
                "Components/App.razor",
                "Components/Routes.razor",
                "Components/Layout/MainLayout.razor",
                "Components/Pages/Home.razor",
                "wwwroot/app.css",
                "README.md",
            ],
            vec![
                ("MutagenGeneratedApp.csproj", "Microsoft.NET.Sdk.Web"),
                ("Program.cs", "MapRazorComponents<App>()"),
            ],
        ),
        (
            "cloudflare-worker",
            vec![
                "package.json",
                "src/index.js",
                "test/index.test.js",
                "scripts/build.mjs",
                "wrangler.toml",
                "README.md",
            ],
            vec![
                (
                    "package.json",
                    "\"dev\": \"wrangler dev src/index.js --local --port 8787\"",
                ),
                ("src/index.js", "createResponse"),
            ],
        ),
    ];

    for (stack, expected_paths, content_checks) in cases {
        let workspace_name = format!("project-scaffold-{stack}");
        let workspace = TestWorkspace::new(&workspace_name);

        init_project(ProjectInitOptions {
            workspace_root: workspace.root.clone(),
            name: format!("{stack} App"),
            stack: stack.to_string(),
            design_system: "plain-css".to_string(),
            deploy_target: None,
            force: false,
        })
        .expect("project init should succeed");
        apply_blueprint(ProjectApplyBlueprintOptions {
            workspace_root: workspace.root.clone(),
            stack: None,
        })
        .expect("blueprint apply should succeed");

        let result = scaffold_project(ProjectScaffoldOptions {
            workspace_root: workspace.root.clone(),
            force: false,
        })
        .expect("project scaffold should succeed");

        assert!(result.ok, "{stack} scaffold should be ok");
        assert_eq!(result.status, "scaffolded");
        assert_eq!(result.stack, stack);

        for path in expected_paths {
            assert!(
                workspace.root.join(path).exists(),
                "{path} should exist for {stack}"
            );
            assert!(
                result.created_paths.contains(&path.to_string()),
                "{path} should be reported as created for {stack}"
            );
        }

        for (path, expected) in content_checks {
            let body =
                fs::read_to_string(workspace.root.join(path)).expect("scaffold file should read");
            assert!(
                body.contains(expected),
                "{path} should contain `{expected}` for {stack}"
            );
        }
    }
}

#[test]
fn project_verify_generated_runs_build_loop_and_stops_preview() {
    let workspace = TestWorkspace::new("project-verify-generated");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "verify-check".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");
    scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace.root.clone(),
        force: false,
    })
    .expect("project scaffold should succeed");
    replace_capsule_command(&workspace.root, "setup", "true");
    replace_capsule_command(&workspace.root, "test", "true");
    replace_capsule_command(&workspace.root, "build", "true");
    replace_capsule_command(&workspace.root, "dev", "sleep 30");
    replace_capsule_preview_url(&workspace.root, "native://test");

    let result = verify_generated_project(ProjectVerifyGeneratedOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project verification should succeed");

    assert!(result.ok);
    assert_eq!(result.status, "verified");
    assert_eq!(
        result
            .steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "inspect",
            "doctor",
            "setup",
            "test",
            "build",
            "preview_start",
            "preview_check",
            "preview_stop",
        ]
    );
    assert!(
        result.steps.iter().all(|step| step.ok),
        "all verification steps should pass"
    );
    assert!(!workspace.root.join(".mutagen/state/preview.json").exists());
}

#[test]
fn project_verify_generated_stops_after_failed_build() {
    let workspace = TestWorkspace::new("project-verify-generated-failure");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "verify-failure".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");
    scaffold_project(ProjectScaffoldOptions {
        workspace_root: workspace.root.clone(),
        force: false,
    })
    .expect("project scaffold should succeed");
    replace_capsule_command(&workspace.root, "setup", "true");
    replace_capsule_command(&workspace.root, "test", "true");
    replace_capsule_command(&workspace.root, "build", "false");
    replace_capsule_command(&workspace.root, "dev", "sleep 30");
    replace_capsule_preview_url(&workspace.root, "native://test");

    let result = verify_generated_project(ProjectVerifyGeneratedOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("project verification should return failure result");

    assert!(!result.ok);
    assert_eq!(result.status, "failed");
    assert_eq!(
        result
            .steps
            .iter()
            .map(|step| step.name.as_str())
            .collect::<Vec<_>>(),
        vec!["inspect", "doctor", "setup", "test", "build"]
    );
    assert_eq!(
        result.steps.last().expect("build step should exist").status,
        "failed"
    );
    assert!(!workspace.root.join(".mutagen/state/preview.json").exists());
}

#[test]
fn project_apply_blueprint_supports_rust_bevy() {
    let workspace = TestWorkspace::new("project-blueprint-bevy");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "tiny-planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");

    let result = apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("rust bevy blueprint should apply");

    assert_eq!(result.capsule.stack, "rust-bevy");
    assert_eq!(result.capsule.commands.setup, "cargo fetch");
    assert_eq!(result.capsule.commands.dev, "cargo run");
    assert_eq!(result.capsule.commands.test, "cargo test");
    assert_eq!(result.capsule.commands.build, "cargo build --release");
    assert_eq!(result.capsule.preview.url, "native://bevy");
    assert_eq!(result.capsule.preview.readiness_timeout_seconds, 120);
}

#[test]
fn project_apply_blueprint_rejects_unknown_stack() {
    let workspace = TestWorkspace::new("project-blueprint-unknown");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "mystery-box".to_string(),
        stack: "unknown-stack".to_string(),
        design_system: "plain".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");

    let error = apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect_err("unknown stack should fail");

    assert!(error.to_string().contains("unsupported stack"));
    assert!(error.to_string().contains("nextjs-postgres"));
}

#[test]
fn project_run_command_dry_run_resolves_blueprint_command() {
    let workspace = TestWorkspace::new("project-run-dry");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "tiny-planet".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    let result = run_project_command(ProjectRunCommandOptions {
        workspace_root: workspace.root.clone(),
        kind: ProjectCommandKind::Build,
        dry_run: true,
    })
    .expect("dry-run command should resolve");

    assert!(result.ok);
    assert_eq!(result.status, "dry_run");
    assert_eq!(result.command, "cargo build --release");
    assert_eq!(result.exit_code, None);
}

#[test]
fn project_run_command_executes_and_logs_result() {
    let workspace = TestWorkspace::new("project-run-command");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "scripted".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    replace_capsule_command(&workspace.root, "test", "printf 'command-ok'");

    let result = run_project_command(ProjectRunCommandOptions {
        workspace_root: workspace.root.clone(),
        kind: ProjectCommandKind::Test,
        dry_run: false,
    })
    .expect("project command should run");

    assert!(result.ok);
    assert_eq!(result.status, "completed");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.stdout, "command-ok");

    let log = fs::read_to_string(workspace.root.join(".mutagen/state/build-log.jsonl"))
        .expect("build log should exist");
    assert!(log.contains("\"event\":\"project_command\""));
    assert!(log.contains("\"command_kind\":\"test\""));
}

#[test]
fn project_run_command_reports_failed_exit() {
    let workspace = TestWorkspace::new("project-run-fail");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "scripted-fail".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    replace_capsule_command(&workspace.root, "test", "printf 'bad' >&2; exit 7");

    let result = run_project_command(ProjectRunCommandOptions {
        workspace_root: workspace.root.clone(),
        kind: ProjectCommandKind::Test,
        dry_run: false,
    })
    .expect("failed project command should still return a result");

    assert!(!result.ok);
    assert_eq!(result.status, "failed");
    assert_eq!(result.exit_code, Some(7));
    assert_eq!(result.stderr, "bad");
}

#[test]
fn project_preview_plan_resolves_dev_command_and_url() {
    let workspace = TestWorkspace::new("project-preview-plan");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "crew-scheduler".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");

    let result = preview_plan(ProjectPreviewPlanOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview plan should resolve");

    assert!(result.ok);
    assert_eq!(result.status, "ready");
    assert_eq!(result.stack, "nextjs-postgres");
    assert_eq!(result.url, "http://localhost:3000");
    assert_eq!(result.command_kind, ProjectCommandKind::Dev);
    assert_eq!(result.command, "npm run dev");
    assert_eq!(result.readiness_timeout_seconds, 60);
}

#[test]
fn project_preview_status_reports_stopped_without_state() {
    let workspace = TestWorkspace::new("project-preview-stopped");

    let result = preview_status(ProjectPreviewLifecycleOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview status should succeed without state");

    assert!(result.ok);
    assert_eq!(result.status, "stopped");
    assert!(!result.running);
    assert!(!result.ready);
    assert_eq!(result.pid, None);
}

#[test]
fn project_preview_start_status_and_stop_manage_process_state() {
    let workspace = TestWorkspace::new("project-preview-lifecycle");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "preview-life".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");
    replace_capsule_command(&workspace.root, "dev", "sleep 30");
    replace_capsule_preview_url(&workspace.root, "native://test");

    let start = preview_start(ProjectPreviewLifecycleOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview start should succeed");

    assert!(start.ok);
    assert_eq!(start.status, "running_ready");
    assert!(start.running);
    assert!(start.ready);
    assert!(start.pid.is_some());
    assert!(workspace.root.join(".mutagen/state/preview.json").exists());

    let status = preview_status(ProjectPreviewLifecycleOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview status should succeed");

    assert!(status.running);
    assert_eq!(status.pid, start.pid);

    let stop = preview_stop(ProjectPreviewLifecycleOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview stop should succeed");

    assert!(stop.ok);
    assert_eq!(stop.status, "stopped");
    assert!(!stop.running);
    assert!(!workspace.root.join(".mutagen/state/preview.json").exists());
}

#[test]
fn project_preview_check_reports_native_running_state() {
    let workspace = TestWorkspace::new("project-preview-check-native");

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "preview-check-native".to_string(),
        stack: "rust-bevy".to_string(),
        design_system: "bevy-ui".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");
    replace_capsule_command(&workspace.root, "dev", "sleep 30");
    replace_capsule_preview_url(&workspace.root, "native://test");

    let start = preview_start(ProjectPreviewLifecycleOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview start should succeed");
    assert!(start.ok);

    let check = preview_check(ProjectPreviewCheckOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview check should succeed");

    assert!(check.ok);
    assert_eq!(check.status, "ready");
    assert_eq!(check.mode, "native");
    assert!(check.running);
    assert!(check.ready);

    preview_stop(ProjectPreviewLifecycleOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview stop should succeed");
}

#[test]
fn project_preview_check_detects_reachable_web_target_without_state() {
    let workspace = TestWorkspace::new("project-preview-check-web");
    let listener = TcpListener::bind("127.0.0.1:0").expect("test port should bind");
    let port = listener
        .local_addr()
        .expect("listener should expose address")
        .port();

    init_project(ProjectInitOptions {
        workspace_root: workspace.root.clone(),
        name: "preview-check-web".to_string(),
        stack: "nextjs-postgres".to_string(),
        design_system: "shadcn".to_string(),
        deploy_target: None,
        force: false,
    })
    .expect("project init should succeed");
    apply_blueprint(ProjectApplyBlueprintOptions {
        workspace_root: workspace.root.clone(),
        stack: None,
    })
    .expect("blueprint apply should succeed");
    replace_capsule_preview_url(&workspace.root, &format!("http://127.0.0.1:{port}"));

    let check = preview_check(ProjectPreviewCheckOptions {
        workspace_root: workspace.root.clone(),
    })
    .expect("preview check should succeed");

    assert!(check.ok);
    assert_eq!(check.status, "reachable_without_state");
    assert_eq!(check.mode, "web");
    assert!(!check.running);
    assert!(check.ready);
}

struct TestWorkspace {
    root: PathBuf,
}

fn replace_capsule_command(root: &std::path::Path, key: &str, command: &str) {
    let path = root.join(".mutagen/project.json");
    let raw = fs::read_to_string(&path).expect("capsule should read");
    let mut capsule: Value = serde_json::from_str(&raw).expect("capsule should parse");
    capsule["commands"][key] = Value::String(command.to_string());
    fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&capsule).expect("capsule should serialize")
        ),
    )
    .expect("capsule should write");
}

fn replace_capsule_preview_url(root: &std::path::Path, url: &str) {
    let path = root.join(".mutagen/project.json");
    let raw = fs::read_to_string(&path).expect("capsule should read");
    let mut capsule: Value = serde_json::from_str(&raw).expect("capsule should parse");
    capsule["preview"]["url"] = Value::String(url.to_string());
    fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::to_string_pretty(&capsule).expect("capsule should serialize")
        ),
    )
    .expect("capsule should write");
}

impl TestWorkspace {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "mutagen-harness-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test workspace should be created");

        Self { root }
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
