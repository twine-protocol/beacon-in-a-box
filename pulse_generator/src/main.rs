use biab_utils::{handle_shutdown_signal, init_logger};
use std::{env, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
  io::{AsyncBufReadExt, BufReader},
  net::TcpListener,
  sync::Notify,
  time::interval,
};

#[tokio::main]
async fn main() {
  // Load environment variables
  let addr: String = env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:5555".to_string());

  init_logger();

  // Setup graceful shutdown
  let shutdown = Arc::new(Notify::new());
  {
    let shutdown = shutdown.clone();

    tokio::spawn(async move {
      handle_shutdown_signal(shutdown).await;
    });
  }

  // Start background task
  tokio::spawn(run_periodic_task(shutdown.clone()));

  // Start TCP server
  start_tcp_server(addr, shutdown.clone()).await;
}

// Periodic background task
async fn run_periodic_task(shutdown: Arc<Notify>) {
  let task_interval: u64 = env::var("TASK_INTERVAL")
    .unwrap_or_else(|_| "5".to_string())
    .parse()
    .unwrap_or(5);
  let mut interval = interval(Duration::from_secs(task_interval));
  loop {
    tokio::select! {
      _ = interval.tick() => {
        log::info!("Running scheduled task...");
        // Simulate work
      }
      _ = shutdown.notified() => {
        log::info!("Stopping tasks...");
        break;
      }
    }
  }
}

// TCP Server to listen for messages
async fn start_tcp_server(addr: String, shutdown: Arc<Notify>) {
  let messenger = biab_utils::Messenger::new();
  let listener = match TcpListener::bind(&addr).await {
    Ok(listener) => listener,
    Err(e) => {
      log::error!("Failed to bind to {}: {}", addr, e);
      panic!("Failed to bind to address");
    }
  };

  log::info!("Listening on {}", addr);

  loop {
    tokio::select! {
      Ok((stream, peer)) = listener.accept() => {
        log::debug!("New connection from {}", peer);
        tokio::spawn(handle_client(messenger.clone(), stream, peer));
      }
      _ = shutdown.notified() => {
        log::info!("Shutting down TCP server...");
        break;
      }
    }
  }
}

async fn handle_client(
  messenger: biab_utils::Messenger,
  mut stream: tokio::net::TcpStream,
  peer: SocketAddr,
) {
  loop {
    if let Some(message) = messenger.receive(&mut stream).await {
      log::debug!("[{}] Received message: {:?}", peer, message);
    }
  }

  // log::debug!("[{}] Connection closed", peer);
}
