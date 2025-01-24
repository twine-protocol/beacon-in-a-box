use std::sync::Arc;
use std::time::Duration;

use biab_utils::{handle_shutdown_signal, init_logger};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Notify;
use tokio::time::interval;

#[tokio::main]
async fn main() -> std::io::Result<()> {
  let mut stream = TcpStream::connect("generator:5555").await?;

  let shutdown = Arc::new(Notify::new());

  init_logger();

  {
    let shutdown = shutdown.clone();
    tokio::spawn(async move {
      handle_shutdown_signal(shutdown).await;
    });
  }

  let mut interval = interval(Duration::from_secs(5));

  let messenger = biab_utils::Messenger::new();

  loop {
    tokio::select! {
      _ = interval.tick() => {
        log::info!("Running scheduled task...");
        // Simulate work
        messenger.send_text(&mut stream, "hello").await?;
      }
      _ = shutdown.notified() => {
        log::info!("Stopping tasks...");
        break;
      }
    }
  }

  Ok(())
}
