use std::collections::HashMap;
use std::sync::Arc;

use naaf_core::span;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tracing::Subscriber;
use tracing_subscriber::Layer;

use crate::ws::event::FrontendEvent;

pub struct WsLayer {
    tx: mpsc::UnboundedSender<FrontendEvent>,
    spans: Arc<Mutex<HashMap<u64, SpanInfo>>>,
}

#[derive(Clone, Debug)]
struct SpanInfo {
    name: String,
    component: Option<String>,
    task_name: Option<String>,
    task_label: Option<String>,
}

struct SpanFieldVisitor<'a> {
    component: &'a mut Option<String>,
    task_name: &'a mut Option<String>,
    task_label: &'a mut Option<String>,
}

struct EventFieldVisitor<'a> {
    action: &'a mut Option<String>,
    attempt: &'a mut Option<u64>,
    accepted: &'a mut Option<bool>,
    finding_count: &'a mut Option<u64>,
    reason: &'a mut Option<String>,
    stage: &'a mut Option<String>,
    message: &'a mut String,
}

impl WsLayer {
    pub fn new(tx: mpsc::UnboundedSender<FrontendEvent>) -> Self {
        Self {
            tx,
            spans: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn send(&self, event: FrontendEvent) {
        let _ = self.tx.send(event);
    }

    fn find_span_info(
        &self,
        id: &tracing::span::Id,
        spans: &HashMap<u64, SpanInfo>,
    ) -> Option<SpanInfo> {
        spans.get(&id.into_u64()).cloned()
    }

    fn should_forward_target(target: &str) -> bool {
        target == env!("CARGO_CRATE_NAME")
            || target.starts_with(concat!(env!("CARGO_CRATE_NAME"), "::"))
    }

    fn should_forward_metadata(metadata: &tracing::Metadata<'_>) -> bool {
        Self::should_forward_target(metadata.target())
            || matches!(
                *metadata.level(),
                tracing::Level::WARN | tracing::Level::ERROR
            )
    }
}

impl<S> Layer<S> for WsLayer
where
    S: Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !Self::should_forward_metadata(attrs.metadata()) {
            return;
        }

        let mut component = None;
        let mut task_name = None;
        let mut task_label = None;

        let mut visitor = SpanFieldVisitor {
            component: &mut component,
            task_name: &mut task_name,
            task_label: &mut task_label,
        };
        attrs.values().record(&mut visitor);

        let name = attrs.metadata().name().to_string();

        let mut spans = self.spans.lock();
        spans.insert(
            id.into_u64(),
            SpanInfo {
                name,
                component,
                task_name,
                task_label,
            },
        );
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if !Self::should_forward_metadata(event.metadata()) {
            return;
        }

        let mut action = None;
        let mut attempt: Option<u64> = None;
        let mut accepted = None;
        let mut finding_count: Option<u64> = None;
        let mut reason = None;
        let mut stage = None;
        let mut message = String::new();

        let mut visitor = EventFieldVisitor {
            action: &mut action,
            attempt: &mut attempt,
            accepted: &mut accepted,
            finding_count: &mut finding_count,
            reason: &mut reason,
            stage: &mut stage,
            message: &mut message,
        };
        event.record(&mut visitor);

        let spans = self.spans.lock();
        let current_span = ctx.event_span(event).or_else(|| ctx.lookup_current());
        let span_info = current_span.as_ref().and_then(
            |span: &tracing_subscriber::registry::SpanRef<'_, S>| {
                self.find_span_info(&span.id(), &spans)
            },
        );

        if let Some(info) = &span_info {
            let task_name = info.task_name.clone().unwrap_or_else(|| info.name.clone());
            let task_label = info.task_label.clone().unwrap_or_else(|| task_name.clone());

            match info.name.as_str() {
                span::name::STEP => {
                    if let Some(action) = action.as_deref() {
                        match action {
                            span::action::RUN_START => {
                                self.send(FrontendEvent::StepStarted {
                                    task_label: task_label.clone(),
                                });
                            }
                            span::action::ATTEMPT_START => {
                                self.send(FrontendEvent::StepAttemptStarted {
                                    task_label: task_label.clone(),
                                    attempt: attempt.unwrap_or(0) as usize,
                                });
                            }
                            span::action::ATTEMPT_VALIDATED => {
                                self.send(FrontendEvent::StepAttemptValidated {
                                    task_label: task_label.clone(),
                                    attempt: attempt.unwrap_or(0) as usize,
                                    accepted: accepted.unwrap_or(false),
                                    finding_count: finding_count.unwrap_or(0) as usize,
                                });
                            }
                            span::action::ATTEMPT_REPAIR_START => {
                                self.send(FrontendEvent::StepRepairStarted {
                                    task_label: task_label.clone(),
                                    attempt: attempt.unwrap_or(0) as usize,
                                });
                            }
                            span::action::RUN_COMPLETE => {
                                self.send(FrontendEvent::StepCompleted {
                                    task_label: task_label.clone(),
                                    attempts: attempt.unwrap_or(1) as usize,
                                });
                            }
                            span::action::RUN_REJECTED => {
                                self.send(FrontendEvent::StepRejected {
                                    task_label: task_label.clone(),
                                    attempts: attempt.unwrap_or(0) as usize,
                                    reason: reason.unwrap_or_else(|| "unknown".to_string()),
                                });
                            }
                            span::action::RUN_ERROR => {
                                self.send(FrontendEvent::StepFailed {
                                    task_label: task_label.clone(),
                                    stage: stage.unwrap_or_else(|| "unknown".to_string()),
                                });
                            }
                            _ => {}
                        }
                    }

                    if !message.is_empty() {
                        self.send(FrontendEvent::Log {
                            level: *event.metadata().level(),
                            target: event.metadata().target().to_string(),
                            message,
                        });
                    }
                }
                span::name::TASK
                | span::name::CHECK
                | span::name::MATERIALISER
                | span::name::REPAIR => {
                    let component = info.component.clone().unwrap_or_default();
                    if let Some(action) = action.as_deref() {
                        match action {
                            span::action::RUN_START => {
                                self.send(FrontendEvent::ComponentStarted {
                                    component,
                                    name: task_label.clone(),
                                });
                            }
                            span::action::RUN_COMPLETE => {
                                self.send(FrontendEvent::ComponentCompleted {
                                    component,
                                    name: task_label.clone(),
                                });
                            }
                            span::action::RUN_ERROR => {
                                self.send(FrontendEvent::ComponentFailed {
                                    component,
                                    name: task_label.clone(),
                                });
                            }
                            _ => {}
                        }
                    }

                    if !message.is_empty() {
                        self.send(FrontendEvent::Log {
                            level: *event.metadata().level(),
                            target: event.metadata().target().to_string(),
                            message,
                        });
                    }
                }
                _ => {
                    if !message.is_empty() {
                        self.send(FrontendEvent::Log {
                            level: *event.metadata().level(),
                            target: event.metadata().target().to_string(),
                            message,
                        });
                    }
                }
            }
        } else if !message.is_empty() {
            self.send(FrontendEvent::Log {
                level: *event.metadata().level(),
                target: event.metadata().target().to_string(),
                message,
            });
        }
    }

    fn on_close(&self, id: tracing::span::Id, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut spans = self.spans.lock();
        spans.remove(&id.into_u64());
    }
}

impl tracing::field::Visit for SpanFieldVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "component" => *self.component = Some(value.to_string()),
            "task" | "check" | "materialiser" | "planner" => {
                *self.task_name = Some(value.to_string());
            }
            "label" => *self.task_label = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, _field: &tracing::field::Field, _value: &dyn std::fmt::Debug) {}
}

impl tracing::field::Visit for EventFieldVisitor<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "action" => *self.action = Some(value.to_string()),
            "reason" => *self.reason = Some(value.to_string()),
            "stage" => *self.stage = Some(value.to_string()),
            "message" => *self.message = value.to_string(),
            _ => {}
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        match field.name() {
            "attempt" => *self.attempt = Some(value),
            "finding_count" => *self.finding_count = Some(value),
            _ => {}
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "accepted" {
            *self.accepted = Some(value);
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            *self.message = format!("{value:?}");
        }
    }
}
