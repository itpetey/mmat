use std::fmt;

use tracing::{Event, Subscriber, field};
use tracing_subscriber::{
    Layer, Registry, filter::LevelFilter, layer::SubscriberExt, registry::LookupSpan,
    util::SubscriberInitExt,
};

use crate::liveview::{EventSender, FrontendEvent};

pub(crate) struct UiLogLayer {
    event_tx: EventSender,
}

#[derive(Default)]
struct EventFieldVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl UiLogLayer {
    pub(crate) fn new(event_tx: EventSender) -> Self {
        Self { event_tx }
    }
}

impl<S> Layer<S> for UiLogLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor.into_message();
        if message.trim().is_empty() {
            return;
        }

        let _ = self.event_tx.send(FrontendEvent::Log {
            level: *metadata.level(),
            target: metadata.target().to_string(),
            message,
        });
    }
}

impl EventFieldVisitor {
    fn into_message(self) -> String {
        let Some(message) = self.message else {
            return self.fields.join(" ");
        };

        if self.fields.is_empty() {
            message
        } else {
            format!("{} {}", message, self.fields.join(" "))
        }
    }

    fn record_value(&mut self, field: &field::Field, value: impl fmt::Display) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }
}

impl field::Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &field::Field, value: &dyn fmt::Debug) {
        self.record_value(field, format_args!("{value:?}"));
    }

    fn record_str(&mut self, field: &field::Field, value: &str) {
        self.record_value(field, value);
    }

    fn record_bool(&mut self, field: &field::Field, value: bool) {
        self.record_value(field, value);
    }

    fn record_i64(&mut self, field: &field::Field, value: i64) {
        self.record_value(field, value);
    }

    fn record_u64(&mut self, field: &field::Field, value: u64) {
        self.record_value(field, value);
    }
}

pub fn init_liveview_tracing(event_tx: EventSender) {
    Registry::default()
        .with(tracing_subscriber::fmt::layer().with_filter(LevelFilter::INFO))
        .with(UiLogLayer::new(event_tx).with_filter(LevelFilter::DEBUG))
        .init();
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use tracing::Level;
    use tracing_subscriber::{Registry, layer::SubscriberExt};

    use crate::liveview::{FrontendEvent, logging::UiLogLayer};

    #[test]
    fn log_layer_forwards_tracing_events_to_frontend_channel() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let subscriber = Registry::default().with(UiLogLayer::new(tx));

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "naaf_core::step", action = "run.start", attempt = 1, "step started");
        });

        let event = rx.try_recv().expect("tracing event should be forwarded");
        assert!(matches!(
            event,
            FrontendEvent::Log {
                level: Level::INFO,
                target,
                message
            } if target == "naaf_core::step"
                && message.contains("step started")
                && message.contains("action=run.start")
                && message.contains("attempt=1")
        ));
    }
}
