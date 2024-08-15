pub use crate::event_stream::EventBuilder;
use crate::event_stream::{EventStream, SpecCompliantEventBuilder};
use futures_core::stream::Stream;

/// Main entrypoint for creating [`crate::Event`] streams
pub trait Eventsource<Builder>: Sized {
    /// Create an event stream from a stream of bytes
    fn eventsource(self, event_builder: Builder) -> EventStream<Self, Builder>;
}

/// Main entrypoint for creating [`crate::Event`] streams that are spec compliant
///
/// Fields ["id", "event", "data", "retry"] are populated from the stream of bytes,
/// any other fields are ignored.
pub trait SpecCompliantEventsource: Sized {
    fn spec_compliant_eventsource(self) -> EventStream<Self, SpecCompliantEventBuilder>;
}

impl<S, B, E, Builder> Eventsource<Builder> for S
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
    Builder: EventBuilder,
{
    fn eventsource(self, event_builder: Builder) -> EventStream<Self, Builder> {
        EventStream::new(self, event_builder)
    }
}

impl<S, B, E> SpecCompliantEventsource for S
where
    S: Stream<Item = Result<B, E>>,
    B: AsRef<[u8]>,
{
    fn spec_compliant_eventsource(self) -> EventStream<Self, SpecCompliantEventBuilder> {
        EventStream::new(self, SpecCompliantEventBuilder::default())
    }
}
