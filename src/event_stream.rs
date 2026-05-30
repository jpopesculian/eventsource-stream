#[cfg(not(feature = "std"))]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::event::Event;
use crate::parser::{is_lf, line, ValidatedRawEventLine};
use core::convert::TryFrom;
use core::fmt;
use core::pin::Pin;
use core::str::Utf8Error;
use core::time::Duration;
use futures_core::stream::Stream;
use futures_core::task::{Context, Poll};
use pin_project_lite::pin_project;

#[derive(Default, Debug)]
struct EventBuilder {
    event: Event,
    is_complete: bool,
}

impl EventBuilder {
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
    ///    The field is ignored.
    fn add(&mut self, line: ValidatedRawEventLine) {
        match line {
            ValidatedRawEventLine::Field(field, val) => {
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
                    _ => {}
                }
            }
            ValidatedRawEventLine::Comment => {}
            ValidatedRawEventLine::Empty => self.is_complete = true,
        }
    }

    /// From the HTML spec
    ///
    /// 1. Set the last event ID string of the event source to the value of the last event ID buffer. The buffer does not get reset, so the last event ID string of the event source remains set to this value until the next time it is set by the server.
    /// 2. If the data buffer is an empty string, set the data buffer and the event type buffer to the empty string and return.
    /// 3. If the data buffer's last character is a U+000A LINE FEED (LF) character, then remove the last character from the data buffer.
    /// 4. Let event be the result of creating an event using MessageEvent, in the relevant Realm of the EventSource object.
    /// 5. Initialize event's type attribute to message, its data attribute to data, its origin attribute to the serialization of the origin of the event stream's final URL (i.e., the URL after redirects), and its lastEventId attribute to the last event ID string of the event source.
    /// 6. If the event type buffer has a value other than the empty string, change the type of the newly created event to equal the value of the event type buffer.
    /// 7. Set the data buffer and the event type buffer to the empty string.
    /// 8. Queue a task which, if the readyState attribute is set to a value other than CLOSED, dispatches the newly created event at the EventSource object.
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
}

#[derive(Debug, Clone, Copy)]
enum EventStreamState {
    NotStarted,
    Started,
    Terminated,
}

impl EventStreamState {
    fn is_terminated(self) -> bool {
        matches!(self, Self::Terminated)
    }
    fn is_started(self) -> bool {
        matches!(self, Self::Started)
    }
}

pin_project! {
    /// A Stream of events
    pub struct EventStream<S> {
        #[pin]
        stream: S,
        buffer: Vec<u8>,
        builder: EventBuilder,
        state: EventStreamState,
        last_event_id: String,
        already_scanned: usize,
    }
}

impl<S> EventStream<S> {
    /// Initialize the EventStream with a Stream
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: Vec::new(),
            builder: EventBuilder::default(),
            state: EventStreamState::NotStarted,
            last_event_id: String::new(),
            already_scanned: 0,
        }
    }

    /// Set the last event ID of the stream. Useful for initializing the stream with a previous
    /// last event ID
    pub fn set_last_event_id(&mut self, id: impl Into<String>) {
        self.last_event_id = id.into();
    }

    /// Get the last event ID of the stream
    pub fn last_event_id(&self) -> &str {
        &self.last_event_id
    }
}

#[derive(Debug)]
pub struct EventStreamError<E> {
    kind: EventStreamErrorKind<E>,
}

impl<E> EventStreamError<E> {
    pub fn is_utf8(&self) -> bool {
        matches!(self.kind, EventStreamErrorKind::Utf8(_))
    }

    pub fn is_transport(&self) -> bool {
        matches!(self.kind, EventStreamErrorKind::Transport(_))
    }

    pub fn as_transport(&self) -> Option<&E> {
        match &self.kind {
            EventStreamErrorKind::Utf8(_) => None,
            EventStreamErrorKind::Transport(transport) => Some(transport),
        }
    }

    pub fn into_transport(self) -> Option<E> {
        match self.kind {
            EventStreamErrorKind::Utf8(_) => None,
            EventStreamErrorKind::Transport(transport) => Some(transport),
        }
    }

    pub fn as_utf8(&self) -> Option<&Utf8Error> {
        match &self.kind {
            EventStreamErrorKind::Utf8(utf8) => Some(utf8),
            EventStreamErrorKind::Transport(_) => None,
        }
    }

    pub fn into_utf8(self) -> Option<Utf8Error> {
        match self.kind {
            EventStreamErrorKind::Utf8(utf8) => Some(utf8),
            EventStreamErrorKind::Transport(_) => None,
        }
    }
}

impl<E> From<EventStreamErrorKind<E>> for EventStreamError<E> {
    fn from(kind: EventStreamErrorKind<E>) -> Self {
        Self { kind }
    }
}

/// Error thrown while parsing an event line
#[derive(Debug, PartialEq)]
enum EventStreamErrorKind<E> {
    /// Source stream is not valid UTF8
    Utf8(Utf8Error),
    /// Underlying source stream error
    Transport(E),
}

impl<E> fmt::Display for EventStreamError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            EventStreamErrorKind::Utf8(err) => f.write_fmt(format_args!("UTF8 error: {}", err)),
            EventStreamErrorKind::Transport(err) => {
                f.write_fmt(format_args!("Transport error: {}", err))
            }
        }
    }
}

#[cfg(feature = "std")]
impl<E> std::error::Error for EventStreamError<E> where E: fmt::Display + fmt::Debug + Send + Sync {}

fn parse_event(
    buffer: &mut Vec<u8>,
    builder: &mut EventBuilder,
    already_scanned: &mut usize,
) -> Result<Option<Event>, Utf8Error> {
    if buffer.is_empty() {
        return Ok(None);
    }
    loop {
        let (rem, next_line) = match line(buffer.as_ref(), *already_scanned) {
            Ok(o) => o,
            Err(resume_from) => {
                *already_scanned = resume_from;
                return Ok(None);
            }
        };
        let validated_line = ValidatedRawEventLine::try_from(next_line)?;
        builder.add(validated_line);
        let consumed = buffer.len() - rem.len();
        let rem = buffer.split_off(consumed);

        *already_scanned = 0;
        *buffer = rem;
        if builder.is_complete {
            if let Some(event) = builder.dispatch() {
                return Ok(Some(event));
            }
        }
    }
}

// bom           = %xFEFF ; U+FEFF BYTE ORDER MARK
/// Byte representation of the BOM
pub(crate) const BOM: &[u8; 3] = &[0xEF, 0xBB, 0xBF];

impl<S, B, E> Stream for EventStream<S>
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
{
    type Item = Result<Event, EventStreamError<E>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        match parse_event(this.buffer, this.builder, this.already_scanned) {
            Ok(Some(event)) => {
                *this.last_event_id = event.id.clone();
                return Poll::Ready(Some(Ok(event)));
            }
            Ok(None) => {}
            Err(e) => return Poll::Ready(Some(Err(EventStreamErrorKind::Utf8(e).into()))),
        }

        if this.state.is_terminated() {
            return Poll::Ready(None);
        }

        loop {
            match this.stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(slice))) => {
                    let slice = slice.as_ref();
                    if slice.is_empty() {
                        continue;
                    }

                    let slice = if this.state.is_started() {
                        slice
                    } else {
                        *this.state = EventStreamState::Started;
                        if slice.starts_with(BOM) {
                            &slice[3..]
                        } else {
                            slice
                        }
                    };
                    this.buffer.extend_from_slice(slice);

                    match parse_event(this.buffer, this.builder, this.already_scanned) {
                        Ok(Some(event)) => {
                            *this.last_event_id = event.id.clone();
                            return Poll::Ready(Some(Ok(event)));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            return Poll::Ready(Some(Err(EventStreamErrorKind::Utf8(e).into())))
                        }
                    }
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(Some(Err(EventStreamErrorKind::Transport(err).into())))
                }
                Poll::Ready(None) => {
                    *this.state = EventStreamState::Terminated;
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(feature = "std"))]
    use alloc::vec;
    use futures::prelude::*;

    #[tokio::test]
    async fn valid_data_fields() {
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data: Hello, world!\n\n"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![Event {
                event: "message".to_string(),
                data: "Hello, world!".to_string(),
                ..Default::default()
            }]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![
                Ok::<_, ()>("data: Hello,"),
                Ok::<_, ()>(" world!\n\n")
            ]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![Event {
                event: "message".to_string(),
                data: "Hello, world!".to_string(),
                ..Default::default()
            }]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![
                Ok::<_, ()>("data: Hello,"),
                Ok::<_, ()>(""),
                Ok::<_, ()>(" world!\n\n")
            ]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![Event {
                event: "message".to_string(),
                data: "Hello, world!".to_string(),
                ..Default::default()
            }]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data: Hello, world!\n"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data: Hello,\ndata: world!\n\n"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![Event {
                event: "message".to_string(),
                data: "Hello,\nworld!".to_string(),
                ..Default::default()
            }]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data: Hello,\n\ndata: world!\n\n"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![
                Event {
                    event: "message".to_string(),
                    data: "Hello,".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: "world!".to_string(),
                    ..Default::default()
                }
            ]
        );
    }

    #[tokio::test]
    async fn spec_examples() {
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data: This is the first message.

data: This is the second message, it
data: has two lines.

data: This is the third message.

"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![
                Event {
                    event: "message".to_string(),
                    data: "This is the first message.".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: "This is the second message, it\nhas two lines.".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: "This is the third message.".to_string(),
                    ..Default::default()
                }
            ]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "event: add
data: 73857293

event: remove
data: 2153

event: add
data: 113411

"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![
                Event {
                    event: "add".to_string(),
                    data: "73857293".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "remove".to_string(),
                    data: "2153".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "add".to_string(),
                    data: "113411".to_string(),
                    ..Default::default()
                }
            ]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data: YHOO
data: +2
data: 10

"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![Event {
                event: "message".to_string(),
                data: "YHOO\n+2\n10".to_string(),
                ..Default::default()
            },]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                ": test stream

data: first event
id: 1

data:second event
id

data:  third event

"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![
                Event {
                    event: "message".to_string(),
                    id: "1".to_string(),
                    data: "first event".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: "second event".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: " third event".to_string(),
                    ..Default::default()
                }
            ]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data

data
data

data:
"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![
                Event {
                    event: "message".to_string(),
                    data: "".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: "\n".to_string(),
                    ..Default::default()
                },
            ]
        );
        assert_eq!(
            EventStream::new(futures::stream::iter(vec![Ok::<_, ()>(
                "data:test

data: test

"
            )]))
            .try_collect::<Vec<_>>()
            .await
            .unwrap(),
            vec![
                Event {
                    event: "message".to_string(),
                    data: "test".to_string(),
                    ..Default::default()
                },
                Event {
                    event: "message".to_string(),
                    data: "test".to_string(),
                    ..Default::default()
                },
            ]
        );
    }

    #[cfg(test)]
    const _: () = {
        fn _assert<T: Send + Sync>() {}
        fn _check<S: Send + Sync>() {
            _assert::<EventStream<S>>();
        }
    };
}
