use core::mem;
use core::time::Duration;
use reqwest::Client;
use tokio::stream::StreamExt;

#[derive(Default, Debug)]
struct Event {
    pub event: Option<String>,
    pub data: Vec<u8>,
    pub id: Option<Vec<u8>>,
    pub retry: Option<Duration>,
}

#[derive(Debug, Clone, Copy)]
enum Field {
    Event,
    Data,
    Id,
    Retry,
}

#[derive(Debug, Copy, Clone)]
enum State {
    ReadingField,
    ReadingValue,
    NextLine,
}

impl Field {
    fn from_bytes(bytes: Vec<u8>) -> Result<Field, ()> {
        let string = String::from_utf8(bytes).map_err(|_| ())?;
        Ok(match string.as_ref() {
            "event" => Field::Event,
            "data" => Field::Data,
            "id" => Field::Id,
            "retry" => Field::Retry,
            _ => return Err(()),
        })
    }
}

impl Event {
    fn set_field(&mut self, field: Option<Field>, mut data: Vec<u8>) -> Result<(), ()> {
        match field {
            Some(Field::Event) => {
                let event = String::from_utf8(data).map_err(|_| ())?;
                self.event = Some(event);
            }
            Some(Field::Data) => {
                self.data.append(&mut data);
            }
            Some(Field::Id) => {
                self.id = Some(data);
            }
            Some(Field::Retry) => {
                let ms = String::from_utf8(data)
                    .map_err(|_| ())
                    .and_then(|string| string.parse().map_err(|_| ()))?;
                self.retry = Some(Duration::from_millis(ms));
            }
            None => {}
        };
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = Client::new()
        .get("http://localhost:7020/notifications")
        .send()
        .await?
        .bytes_stream();

    let mut buffer = Vec::<u8>::default();
    let mut field: Option<Field> = None;
    let mut event = Event::default();
    let mut state = State::ReadingField;
    while let Some(chunk) = stream.next().await {
        for byte in chunk? {
            match byte {
                b'\n' => match state {
                    State::ReadingField => {
                        // ignore error
                        buffer.clear();
                        field = None;
                        state = State::NextLine;
                    }
                    State::ReadingValue => {
                        // ignore error
                        let _ = event.set_field(mem::take(&mut field), mem::take(&mut buffer));
                        state = State::NextLine;
                    }
                    State::NextLine => {
                        // TODO yield event
                        println!("event finished: {:?}", event);
                        buffer.clear();
                        field = None;
                        event = Event::default();
                        state = State::ReadingField;
                    }
                },
                b':' => match state {
                    State::ReadingField => {
                        // ignore error
                        if let Ok(next_field) = Field::from_bytes(mem::take(&mut buffer)) {
                            field = Some(next_field);
                        }
                        buffer.clear();
                        state = State::ReadingValue;
                    }
                    State::ReadingValue => {
                        buffer.push(byte);
                    }
                    State::NextLine => {
                        state = State::ReadingValue;
                    }
                },
                byte => {
                    if matches!(state, State::NextLine) {
                        state = State::ReadingField;
                    }
                    buffer.push(byte);
                }
            }
        }
    }
    Ok(())
}
