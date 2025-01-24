use std::{env, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
  io::{AsyncBufReadExt, BufReader},
  net::TcpListener,
  signal,
  sync::Notify,
  time::interval,
};

#[tokio::main]
async fn main() {
  // Load environment variables
  let addr: String = env::var("DAEMON_ADDR").unwrap_or_else(|_| "0.0.0.0:5555".to_string());
  let task_interval: u64 = env::var("TASK_INTERVAL")
    .unwrap_or_else(|_| "5".to_string())
    .parse()
    .unwrap_or(5);

  // Setup graceful shutdown
  let shutdown_notify = Arc::new(Notify::new());
  let shutdown_clone = shutdown_notify.clone();

  tokio::spawn(async move {
    handle_shutdown_signal(shutdown_clone).await;
  });

  // Start background task
  tokio::spawn(run_periodic_task(task_interval, shutdown_notify.clone()));

  // Start TCP server
  start_tcp_server(addr, shutdown_notify.clone()).await;
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

// Periodic background task
async fn run_periodic_task(interval_secs: u64, shutdown: Arc<Notify>) {
  let mut interval = interval(Duration::from_secs(interval_secs));
  loop {
    tokio::select! {
      _ = interval.tick() => {
        println!("Running scheduled task...");
        // Simulate work
      }
      _ = shutdown.notified() => {
        println!("Stopping background task...");
        break;
      }
    }
  }
}

// TCP Server to listen for messages
async fn start_tcp_server(addr: String, shutdown: Arc<Notify>) {
  let listener = TcpListener::bind(&addr)
    .await
    .expect("Failed to bind TCP listener");
  println!("Listening on {}", addr);

  loop {
    tokio::select! {
      Ok((stream, peer)) = listener.accept() => {
        println!("New connection from {}", peer);
        tokio::spawn(handle_client(stream, peer));
      }
      _ = shutdown.notified() => {
        println!("Shutting down TCP server...");
        break;
      }
    }
  }
}

async fn handle_client(stream: tokio::net::TcpStream, peer: SocketAddr) {
  let reader = BufReader::new(stream);
  let mut lines = reader.lines();

  while let Ok(Some(line)) = lines.next_line().await {
    println!("[{}] Received: {}", peer, line);
  }

  println!("[{}] Connection closed", peer);
}
