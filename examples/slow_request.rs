// taken from https://github.com/Aaron1011/slow-eventsource-stream/blob/master/src/main.rs and thrown here so I can flame graph it

use eventsource_stream2::{Event, EventStream};
use futures::{stream, TryStreamExt};
use std::env;

#[tokio::main]
async fn main() {
    let num_chunks: usize = env::args()
        .nth(1)
        .expect("Usage: slow-eventsource-stream <num_chunks>")
        .parse()
        .expect("num_chunks must be a positive integer");

    const DATA_SIZE: usize = 8 * 1024 * 1024; // 8 MB

    let expected_data: String = "x".repeat(DATA_SIZE);
    let full_message = format!("data: {}\n\n", expected_data);

    #[allow(clippy::incompatible_msrv)]
    // this example isn't even intended to be a "real" example just a way for me to benchmark
    let chunk_size = full_message.len().div_ceil(num_chunks);

    let chunks: Vec<Result<String, ()>> = full_message
        .as_bytes()
        .chunks(chunk_size)
        .map(|c| Ok(String::from_utf8(c.to_vec()).unwrap()))
        .collect();

    let stream = stream::iter(chunks);

    let events: Vec<Event> = EventStream::new(stream).try_collect().await.unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "message");
    assert_eq!(events[0].data.len(), DATA_SIZE);
    assert_eq!(events[0].data, expected_data);
}
