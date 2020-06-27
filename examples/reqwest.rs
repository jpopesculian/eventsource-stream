use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use reqwest::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = Client::new()
        .get("http://localhost:7020/notifications")
        .send()
        .await?
        .bytes_stream()
        .eventsource();

    while let Some(thing) = stream.next().await {
        println!("{:?}", thing);
    }

    Ok(())
}
