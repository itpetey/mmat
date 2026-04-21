use std::{path::Path, rc::Rc};

use super::{
    AppError, AppRuntime, ImplementationDraft, ImplementationExecutionInput, ManagedPhase,
    PhaseExecutionInput, PhaseReviewResult,
};
use naaf_core::{
    Attempt, CheckExt, MaterialiserExt, NeverFinding, RepairPlannerExt, RetryPolicy, Step, TaskExt,
    check_fn, materialiser_fn, repair_fn, task_fn,
};
use naaf_llm::{CompletionRequest, Message};
use naaf_workspace::prepare_worktree as naaf_prepare_worktree;
use serde::de::DeserializeOwned;

use crate::{
    models::{
        FinalReview, FinalReviewInput, ImplementationDelta, ImplementationManagementRequest,
        ImplementationWorklist,
    },
    prompts::{
        contract_validation_system_prompt, contract_validation_user_prompt,
        final_review_system_prompt, final_review_user_prompt,
        implementation_management_system_prompt, implementation_management_user_prompt,
        implementation_task_system_prompt, implementation_task_user_prompt,
        peer_review_system_prompt, peer_review_user_prompt,
    },
    runtime::WebSearchConfig,
    workflow::execution::AppAgent,
};

pub fn build_agent(
    project_root: &Path,
    web_search: Option<WebSearchConfig>,
    allow_question: bool,
) -> Result<AppAgent, AppError> {
    super::execution::build_agent(project_root, web_search, allow_question)
}

pub fn build_implementation_step(
    model: &str,
    web_search: Option<WebSearchConfig>,
) -> Result<
    Step<
        AppRuntime,
        ImplementationExecutionInput,
        ImplementationDraft,
        crate::models::StageFinding,
        AppError,
    >,
    AppError,
> {
    let model_for_task = model.to_string();
    let model_for_review = model.to_string();
    let task_web_search = web_search.clone();
    let review_web_search = web_search.clone();
    let task = task_fn(
        move |runtime: &AppRuntime, input: ImplementationExecutionInput| {
            let model = model_for_task.clone();
            let web_search = task_web_search.clone();
            Box::pin(async move {
                let worktree_root =
                    prepare_worktree(runtime.project_root(), &input.worktree_name).await?;
                let llm = build_agent(&worktree_root, web_search, false)?;
                let request = CompletionRequest::new(
                    model,
                    vec![
                        Message::system(implementation_task_system_prompt()),
                        Message::user(implementation_task_user_prompt(&input.task)?),
                    ],
                );
                let delta = execute_json_stage::<ImplementationDelta>(
                    &llm,
                    runtime,
                    request,
                    "implementation task",
                )
                .await?;
                Ok::<_, AppError>(ImplementationDraft {
                    input: input.task,
                    worktree_name: input.worktree_name,
                    delta,
                })
            })
        },
    )
    .observed_as("implement_item");

    let apply_deltas = materialiser_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            super::execution::apply_file_deltas(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                &draft.delta,
            )?;
            Ok::<_, AppError>(draft)
        })
    })
    .observed_as("apply_file_deltas");

    let cargo_fmt = materialiser_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            super::execution::run_command(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo fmt --all",
                &["fmt", "--all"],
            )
            .await?;
            Ok::<_, AppError>(draft)
        })
    })
    .observed_as("cargo_fmt");

    let cargo_check = check_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            super::execution::run_validator(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo check",
                &["check"],
            )
            .await
        })
    })
    .observed_as("cargo_check");

    let cargo_test = check_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            super::execution::run_validator(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo test",
                &["test"],
            )
            .await
        })
    })
    .observed_as("cargo_test");

    let cargo_clippy = check_fn(|runtime: &AppRuntime, draft: ImplementationDraft| {
        Box::pin(async move {
            super::execution::run_validator(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                "cargo clippy -- -D warnings",
                &["clippy", "--", "-D", "warnings"],
            )
            .await
        })
    })
    .observed_as("cargo_clippy");

    let peer_review = check_fn(move |runtime: &AppRuntime, draft: ImplementationDraft| {
        let model = model_for_review.clone();
        let web_search = review_web_search.clone();
        Box::pin(async move {
            let llm = build_agent(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                web_search,
                false,
            )?;
            let request = CompletionRequest::new(
                model,
                vec![
                    Message::system(peer_review_system_prompt()),
                    Message::user(peer_review_user_prompt(&draft.input, &draft.delta)?),
                ],
            );
            let review = execute_json_stage::<crate::models::StageReview>(
                &llm,
                runtime,
                request,
                "peer review",
            )
            .await?;
            Ok::<_, AppError>(review.findings)
        })
    })
    .observed_as("peer_review");

    let contract_validation_model = model.to_string();
    let contract_validation_web_search = web_search;
    let contract_validation = check_fn(move |runtime: &AppRuntime, draft: ImplementationDraft| {
        let model = contract_validation_model.clone();
        let web_search = contract_validation_web_search.clone();
        Box::pin(async move {
            let llm = build_agent(
                mmat_worktree_path(runtime.project_root(), &draft.worktree_name).as_path(),
                web_search,
                false,
            )?;
            let request = CompletionRequest::new(
                model,
                vec![
                    Message::system(contract_validation_system_prompt()),
                    Message::user(contract_validation_user_prompt(&draft.input, &draft.delta)?),
                ],
            );
            let review = execute_json_stage::<crate::models::StageReview>(
                &llm,
                runtime,
                request,
                "contract validation",
            )
            .await?;
            Ok::<_, AppError>(review.findings)
        })
    })
    .observed_as("contract_validation");

    let revise = repair_fn(
        |_runtime: &AppRuntime,
         attempts: Vec<
            Attempt<ImplementationExecutionInput, ImplementationDraft, crate::models::StageFinding>,
        >| {
            Box::pin(async move {
                let Some(last) = attempts.last() else {
                    return Err(AppError::Workflow(
                        "implementation repair was invoked without a prior attempt".to_string(),
                    ));
                };

                let mut prior_feedback = Vec::new();
                for attempt in &attempts {
                    prior_feedback.extend(attempt.findings.clone());
                }

                let mut next_input = last.input.clone();
                next_input.task.prior_feedback = prior_feedback;
                Ok::<_, AppError>(next_input)
            })
        },
    )
    .observed_as("revise_item");

    Ok(Step::builder(task)
        .materialise(apply_deltas)
        .materialise(cargo_fmt)
        .validate(cargo_check)
        .validate(cargo_test)
        .validate(cargo_clippy)
        .validate(peer_review)
        .validate(contract_validation)
        .repair_with(revise)
        .retry_policy(RetryPolicy::new(super::IMPLEMENTATION_RETRY_LIMIT))
        .build())
}

pub fn build_managed_phase_step(
    project_root: &Path,
    model: &str,
    web_search_enabled: bool,
    web_search: Option<WebSearchConfig>,
) -> Result<
    Step<AppRuntime, ImplementationManagementRequest, ManagedPhase, NeverFinding, AppError>,
    AppError,
> {
    let llm = Rc::new(build_agent(project_root, web_search, false)?);
    let model = model.to_string();
    let system_prompt = implementation_management_system_prompt(web_search_enabled);
    let task = task_fn(
        move |runtime: &AppRuntime, request: ImplementationManagementRequest| {
            let llm = llm.clone();
            let model = model.clone();
            let system_prompt = system_prompt.clone();
            Box::pin(async move {
                let worklist = execute_json_stage::<ImplementationWorklist>(
                    llm.as_ref(),
                    runtime,
                    CompletionRequest::new(
                        model,
                        vec![
                            Message::system(system_prompt),
                            Message::user(implementation_management_user_prompt(&request)?),
                        ],
                    ),
                    "implementation management",
                )
                .await?;
                super::logging::log_worklist_summary(runtime, &worklist)?;
                Ok::<_, AppError>(ManagedPhase { request, worklist })
            })
        },
    )
    .observed_as("implementation_management");

    Ok(Step::builder(task).with_findings::<NeverFinding>().build())
}

pub fn build_phase_review_step(
    project_root: &Path,
    model: &str,
    web_search_enabled: bool,
    web_search: Option<WebSearchConfig>,
) -> Result<
    Step<AppRuntime, PhaseExecutionInput, PhaseReviewResult, NeverFinding, AppError>,
    AppError,
> {
    use crate::artifacts::RunArtifact;
    use naaf_workspace::{
        create_baseline_snapshot as naaf_create_baseline_snapshot,
        remove_directory_if_exists as naaf_remove_directory_if_exists,
        sync_workspace_state as naaf_sync_workspace_state,
    };

    let llm = Rc::new(build_agent(project_root, web_search, false)?);
    let model = model.to_string();
    let system_prompt = final_review_system_prompt(web_search_enabled);
    let task = task_fn(move |runtime: &AppRuntime, input: PhaseExecutionInput| {
        let llm = llm.clone();
        let model = model.clone();
        let system_prompt = system_prompt.clone();
        Box::pin(async move {
            let baseline_root = naaf_create_baseline_snapshot(
                runtime.project_root(),
                &format!("baseline-{}", input.phase.request.phase.replace('_', "-")),
            )
            .map_err(|error| AppError::Workspace(error.to_string()))?;

            let cleanup_root = baseline_root.clone();
            let project_root = runtime.project_root().to_path_buf();
            let result = async {
                for draft in &input.drafts {
                    super::execution::merge_item_worktree(runtime.project_root(), &baseline_root, draft).await?;
                }

                let task_cards: Vec<_> = input
                    .drafts
                    .iter()
                    .map(|d| d.input.work_item.clone())
                    .collect();
                let commands_run =
                    super::execution::run_phase_verification_commands(runtime.project_root(), &task_cards).await;

                let mut completed_items = input.phase.request.completed_items.clone();
                let current_results = super::execution::build_item_results(
                    &input.drafts,
                    runtime.project_root(),
                    &baseline_root,
                    commands_run,
                )?;

                for result in &current_results {
                    super::logging::log_implementation_result(runtime, result)?;
                    runtime.persist_task_result(result)?;
                }
                completed_items.extend(current_results);
                runtime.persist_artifact(
                    RunArtifact::EvidenceLog,
                    &crate::models::EvidenceLog {
                        task_results: completed_items.clone(),
                    },
                )?;

                let review = execute_json_stage::<FinalReview>(
                    llm.as_ref(),
                    runtime,
                    CompletionRequest::new(
                        model,
                        vec![
                            Message::system(system_prompt),
                            Message::user(final_review_user_prompt(&FinalReviewInput {
                                approved: input.phase.request.approved.clone(),
                                plan: input.phase.request.plan.clone(),
                                completed_items: completed_items.clone(),
                            })?),
                        ],
                    ),
                    "final review",
                )
                .await?;
                super::logging::log_final_review_summary(runtime, &review)?;
                match super::execution::phase_review_action(input.phase.request.pass_index, &review) {
                    super::PhaseReviewAction::Remediate => runtime.log_warn(
                        "Final review requested remediation. Spawning another implementation management phase.",
                    )?,
                    super::PhaseReviewAction::Halt if review.remediation_items.is_empty() => runtime.log_warn(
                        "Final review is not yet ready but did not provide remediation items. Stopping the workflow.",
                    )?,
                    super::PhaseReviewAction::Halt => runtime.log_warn(
                        "Maximum remediation passes reached. Stopping the workflow.",
                    )?,
                    super::PhaseReviewAction::Complete => {}
                }
                Ok::<_, AppError>(PhaseReviewResult {
                    phase: input.phase,
                    completed_items,
                    review,
                })
            }
            .await;

            let mut errors = Vec::new();
            if result.is_err()
                && let Err(error) = naaf_sync_workspace_state(&cleanup_root, &project_root)
            {
                errors.push(format!("workspace rollback failed: {error}"));
            }
            if let Err(error) = naaf_remove_directory_if_exists(&cleanup_root) {
                errors.push(format!("baseline cleanup failed: {error}"));
            }

            if errors.is_empty() {
                result
            } else {
                let context = if result.is_ok() {
                    "phase review succeeded but recovery failed".to_string()
                } else {
                    match &result {
                        Ok(_) => unreachable!(),
                        Err(e) => format!("phase review failed; {e}"),
                    }
                };
                Err(AppError::Workspace(format!(
                    "{context}; recovery errors: {}",
                    errors.join("; ")
                )))
            }
        })
    })
    .observed_as("phase_review");

    Ok(Step::builder(task).with_findings::<NeverFinding>().build())
}

async fn execute_json_stage<T>(
    llm: &AppAgent,
    runtime: &AppRuntime,
    request: CompletionRequest,
    stage: &str,
) -> Result<T, AppError>
where
    T: DeserializeOwned,
{
    super::execution::execute_json_stage(llm, runtime, request, stage).await
}

fn mmat_worktree_path(project_root: &Path, worktree_name: &str) -> std::path::PathBuf {
    project_root.join(super::WORKTREE_DIR).join(worktree_name)
}

async fn prepare_worktree(
    project_root: &Path,
    worktree_name: &str,
) -> Result<std::path::PathBuf, AppError> {
    naaf_prepare_worktree(project_root, worktree_name)
        .await
        .map_err(|error| AppError::Workspace(error.to_string()))
}
