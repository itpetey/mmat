use std::fmt;

#[derive(Debug)]
pub enum FrontendEvent {
    StepStarted {
        task_label: String,
    },
    StepAttemptStarted {
        task_label: String,
        attempt: usize,
    },
    StepAttemptValidated {
        task_label: String,
        attempt: usize,
        accepted: bool,
        finding_count: usize,
    },
    StepRepairStarted {
        task_label: String,
        attempt: usize,
    },
    StepCompleted {
        task_label: String,
        attempts: usize,
    },
    StepRejected {
        task_label: String,
        attempts: usize,
        reason: String,
    },
    StepFailed {
        task_label: String,
        stage: String,
    },
    ComponentStarted {
        component: String,
        name: String,
    },
    ComponentCompleted {
        component: String,
        name: String,
    },
    ComponentFailed {
        component: String,
        name: String,
    },
    Log {
        level: tracing::Level,
        target: String,
        message: String,
    },
    HumanPrompt {
        question: String,
        choices: Vec<String>,
        reply: tokio::sync::oneshot::Sender<String>,
    },
    Quit,
}

impl fmt::Display for FrontendEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StepStarted { task_label, .. } => write!(f, "step started: {task_label}"),
            Self::StepAttemptStarted {
                task_label,
                attempt,
                ..
            } => write!(f, "attempt {attempt} started: {task_label}"),
            Self::StepAttemptValidated {
                task_label,
                attempt,
                accepted,
                finding_count,
                ..
            } => write!(
                f,
                "attempt {attempt} validated: {task_label} (accepted={accepted}, findings={finding_count})"
            ),
            Self::StepRepairStarted {
                task_label,
                attempt,
                ..
            } => write!(f, "repair started: {task_label} (attempt {attempt})"),
            Self::StepCompleted {
                task_label,
                attempts,
                ..
            } => write!(f, "step completed: {task_label} ({attempts} attempts)"),
            Self::StepRejected {
                task_label,
                attempts,
                reason,
                ..
            } => write!(
                f,
                "step rejected: {task_label} ({attempts} attempts, {reason})"
            ),
            Self::StepFailed {
                task_label, stage, ..
            } => write!(f, "step failed: {task_label} ({stage})"),
            Self::ComponentStarted { component, name } => write!(f, "{component} started: {name}"),
            Self::ComponentCompleted { component, name } => {
                write!(f, "{component} completed: {name}")
            }
            Self::ComponentFailed { component, name } => write!(f, "{component} failed: {name}"),
            Self::Log { level, message, .. } => write!(f, "[{level}] {message}"),
            Self::HumanPrompt { question, .. } => write!(f, "? {question}"),
            Self::Quit => write!(f, "quit"),
        }
    }
}
