use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::signal;
use tokio::sync::Notify;
use tokio::time::interval;

#[tokio::main]
async fn main() -> std::io::Result<()> {
  let mut stream = TcpStream::connect("generator:5555").await?;

  let shutdown = Arc::new(Notify::new());

  {
    let shutdown = shutdown.clone();
    tokio::spawn(async move {
      handle_shutdown_signal(shutdown).await;
    });
  }

  let mut interval = interval(Duration::from_secs(5));

  loop {
    tokio::select! {
      _ = interval.tick() => {
        println!("Running scheduled task...");
        // Simulate work
        stream.write_all(b"Hello, world!\n").await?;
      }
      _ = shutdown.notified() => {
        println!("Stopping background task...");
        break;
      }
    }
  }

  Ok(())
}

// Handle graceful shutdown on SIGTERM/SIGINT
async fn handle_shutdown_signal(shutdown: Arc<Notify>) {
  use tokio::signal::unix::{signal, SignalKind};
  let mut sigterm = signal(SignalKind::terminate()).unwrap();
  tokio::select! {
    _ = signal::ctrl_c() => {
      println!("Received shutdown signal, stopping...");
      shutdown.notify_waiters();
    }
    // sigterm
    _ = sigterm.recv() => {
      println!("Received SIGTERM, stopping...");
      shutdown.notify_waiters();
    }
  };
}
