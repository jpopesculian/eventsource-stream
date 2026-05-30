# eventsource-stream2

This is a fork of `eventsource-stream` by Julian Popescu, it's intended to be as close to drop-in as is reasonable although I have taken a couple of liberties around the removal of `nom` as it is not used to drive the parser anymore. If you relied on the `EventStreamError::Parser` variant you'll need to remove that since the parser can no longer error. Original readme is below.

A basic building block for building an Eventsource from a Stream of bytes array like objects. To
learn more about Server Sent Events (SSE) take a look at [the MDN
docs](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events)

## Example

```rust
let mut stream = reqwest::Client::new()
    .get("http://localhost:7020/notifications")
    .send()
    .await?
    .bytes_stream()
    .eventsource();

while let Some(thing) = stream.next().await {
   println!("{:?}", thing);
}
```

License: MIT OR Apache-2.0
