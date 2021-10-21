use eventsource_stream::Eventsource;
use futures::stream::StreamExt;
use reqwest::Client;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut stream = Client::new()
        .get("http://localhost:7020/notifications")
        .send()
        .await?
        .bytes_stream()
        .eventsource();

    while let Some(event) = stream.next().await {
        match event {
            Ok(event) => println!(
                "received: {:?}: {}",
                event.event,
                String::from_utf8_lossy(&event.data)
            ),
            Err(e) => eprintln!("error occured: {}", e),
        }
    }

    Ok(())
}
