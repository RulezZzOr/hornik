use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSeverity {
    Info,
    Warn,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub seq: u64,
    pub uptime_seconds: u64,
    pub severity: EventSeverity,
    pub event_type: String,
    pub component: String,
    pub message: String,
    pub details: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventsResponse {
    pub events: Vec<EventRecord>,
}

#[derive(Debug, Clone, Default)]
pub struct EventBuilder {
    next_seq: u64,
    uptime_seconds: u64,
    events: Vec<EventRecord>,
}

impl EventBuilder {
    pub fn new(uptime_seconds: u64) -> Self {
        Self {
            next_seq: 1,
            uptime_seconds,
            events: Vec::new(),
        }
    }

    pub fn push(
        &mut self,
        severity: EventSeverity,
        event_type: impl Into<String>,
        component: impl Into<String>,
        message: impl Into<String>,
        details: Value,
    ) {
        self.events.push(EventRecord {
            seq: self.next_seq,
            uptime_seconds: self.uptime_seconds,
            severity,
            event_type: event_type.into(),
            component: component.into(),
            message: message.into(),
            details,
        });
        self.next_seq += 1;
    }

    pub fn finish(self) -> EventsResponse {
        EventsResponse {
            events: self.events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builder_assigns_monotonic_sequences() {
        let mut builder = EventBuilder::new(42);
        builder.push(
            EventSeverity::Info,
            "boot.completed",
            "supervisor",
            "boot completed",
            json!({}),
        );
        builder.push(
            EventSeverity::Warn,
            "pool.unconfigured",
            "pool",
            "no pools configured",
            json!({}),
        );

        let events = builder.finish().events;
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[0].uptime_seconds, 42);
    }
}
