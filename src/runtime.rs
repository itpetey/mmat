use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

use futures::future::LocalBoxFuture;
use naaf_llm::{HumanAnswer, HumanIO, HumanQuestion};
use parking_lot::Mutex;

use crate::workflow_old::{WorkflowError, WorkflowStageId};

pub trait StagePromptProvider {
    fn system_prompt_for_stage(&self, stage: WorkflowStageId) -> String;
}

pub trait WorkflowRuntime: HumanIO<Error = WorkflowError> + StagePromptProvider {}

#[derive(Clone, Debug, Default)]
pub struct ScriptedRuntime {
    answers: Arc<Mutex<VecDeque<String>>>,
    asked_questions: Arc<Mutex<Vec<HumanQuestion>>>,
    stage_prompts: Arc<Mutex<BTreeMap<WorkflowStageId, String>>>,
}

impl StagePromptProvider for ScriptedRuntime {
    fn system_prompt_for_stage(&self, stage: WorkflowStageId) -> String {
        self.stage_prompts
            .lock()
            .get(&stage)
            .cloned()
            .unwrap_or_else(|| stage.default_system_prompt())
    }
}

impl<T> WorkflowRuntime for T where T: HumanIO<Error = WorkflowError> + StagePromptProvider {}

impl ScriptedRuntime {
    pub fn new(answers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            answers: Arc::new(Mutex::new(
                answers.into_iter().map(Into::into).collect::<VecDeque<_>>(),
            )),
            asked_questions: Arc::new(Mutex::new(Vec::new())),
            stage_prompts: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn with_stage_prompt(self, stage: WorkflowStageId, prompt: impl Into<String>) -> Self {
        self.stage_prompts.lock().insert(stage, prompt.into());
        self
    }

    pub fn asked_questions(&self) -> Vec<HumanQuestion> {
        self.asked_questions.lock().clone()
    }
}

impl HumanIO for ScriptedRuntime {
    type Error = WorkflowError;

    fn ask<'a>(
        &'a self,
        question: HumanQuestion,
    ) -> LocalBoxFuture<'a, Result<HumanAnswer, Self::Error>> {
        Box::pin(async move {
            self.asked_questions.lock().push(question);
            let answer =
                self.answers.lock().pop_front().ok_or_else(|| {
                    WorkflowError::Human("no scripted answer available".to_string())
                })?;
            Ok(HumanAnswer { content: answer })
        })
    }
}
