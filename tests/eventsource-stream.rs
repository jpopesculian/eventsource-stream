use std::time::Duration;

use eventsource_stream::{is_lf, Event, EventBuilder, RawEventLine};
use eventsource_stream::{Eventsource, SpecCompliantEventsource};
use futures::stream::StreamExt;
use http::response::Builder;
use reqwest::Response;
use reqwest::ResponseBuilderExt;
use url::Url;

#[tokio::test]
async fn populate_fields() {
    let url = Url::parse("https://example.com").unwrap();
    let response = Builder::new()
        .status(200)
        .url(url.clone())
        .body(
            "

:

event: my-event\r\ndata:line1
data: line2
:
id: my-id
:should be ignored too\rretry:42

data:second

data:ignored
",
        )
        .unwrap();
    let response = Response::from(response);
    let mut stream = response.bytes_stream().spec_compliant_eventsource();

    let event = stream.next().await.unwrap().unwrap();
    assert_eq!("my-event", event.event);
    assert_eq!(
        "line1
line2",
        event.data
    );
    assert_eq!("my-id", event.id);
    assert_eq!(std::time::Duration::from_millis(42), event.retry.unwrap());

    let event = stream.next().await.unwrap().unwrap();
    assert_eq!("message", event.event);
    assert_eq!("second", event.data);

    let event = stream.next().await;
    assert!(event.is_none());
}

#[derive(Default, Debug)]
pub struct CustomEventBuilder {
    event: Event,
    is_complete: bool,
}

impl EventBuilder for CustomEventBuilder {
    /// From the HTML spec
    ///
    /// -> If the field name is "event"
    ///    Set the event type buffer to field value.
    ///
    /// -> If the field name is "data"
    ///    Append the field value to the data buffer, then append a single U+000A LINE FEED (LF)
    ///    character to the data buffer.
    ///
    /// -> If the field name is "id"
    ///    If the field value does not contain U+0000 NULL, then set the last event ID buffer
    ///    to the field value. Otherwise, ignore the field.
    ///
    /// -> If the field name is "retry"
    ///    If the field value consists of only ASCII digits, then interpret the field value as
    ///    an integer in base ten, and set the event stream's reconnection time to that integer.
    ///    Otherwise, ignore the field.
    ///
    /// -> Otherwise
    ///    The field is treated as a custom event
    ///    Set the event type buffer to the field name
    ///    Set the data buffer to the field value
    fn add(&mut self, line: RawEventLine) {
        match line {
            RawEventLine::Field(field, val) => {
                let val = val.unwrap_or("");
                match field {
                    "event" => {
                        self.event.event = val.to_string();
                    }
                    "data" => {
                        self.event.data.push_str(val);
                        self.event.data.push('\u{000A}');
                    }
                    "id" => {
                        if !val.contains('\u{0000}') {
                            self.event.id = val.to_string()
                        }
                    }
                    "retry" => {
                        if let Ok(val) = val.parse::<u64>() {
                            self.event.retry = Some(Duration::from_millis(val))
                        }
                    }
                    other => {
                        self.event.event = other.to_string();
                        self.event.data = val.to_string();
                    }
                }
            }
            RawEventLine::Comment(_) => {}
            RawEventLine::Empty => self.is_complete = true,
        }
    }

    fn dispatch(&mut self) -> Option<Event> {
        let builder = core::mem::take(self);
        let mut event = builder.event;
        self.event.id = event.id.clone();

        if event.data.is_empty() {
            return None;
        }

        if is_lf(event.data.chars().next_back().unwrap()) {
            event.data.pop();
        }

        if event.event.is_empty() {
            event.event = "message".to_string();
        }

        Some(event)
    }

    fn is_complete(&self) -> bool {
        self.is_complete
    }
}

#[tokio::test]
async fn populate_fields_non_compliant() {
    let url = Url::parse("https://example.com").unwrap();
    let response = Builder::new()
        .status(200)
        .url(url.clone())
        .body(
            "
data: {\"my\": \"data\"}

custom_field: custom_data

data: {\"my\": \"data2\"}

different_field: different_data

",
        )
        .unwrap();
    let response = Response::from(response);
    let mut stream = response
        .bytes_stream()
        .eventsource(CustomEventBuilder::default());

    let event = stream.next().await.unwrap().unwrap();
    assert_eq!("message", event.event);
    assert_eq!("{\"my\": \"data\"}", event.data);

    let event = stream.next().await.unwrap().unwrap();
    assert_eq!("custom_field", event.event);
    assert_eq!("custom_data", event.data);

    let event = stream.next().await.unwrap().unwrap();
    assert_eq!("message", event.event);
    assert_eq!("{\"my\": \"data2\"}", event.data);

    let event = stream.next().await.unwrap().unwrap();
    assert_eq!("different_field", event.event);
    assert_eq!("different_data", event.data);

    let event = stream.next().await;
    assert!(event.is_none());
}
