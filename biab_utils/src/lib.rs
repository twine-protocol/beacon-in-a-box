// Handle graceful shutdown on SIGTERM/SIGINT
use std::sync::Arc;
use tokio::sync::Notify;

mod tcp_server;
pub use tcp_server::*;

mod messages;
pub use messages::*;

pub async fn handle_shutdown_signal(shutdown: Arc<Notify>) {
  use tokio::signal::{
    ctrl_c,
    unix::{signal, SignalKind},
  };
  let mut sigterm = signal(SignalKind::terminate()).unwrap();
  tokio::select! {
    _ = ctrl_c() => {
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

pub fn init_logger() {
  use simple_logger::SimpleLogger;
  let level = match std::env::var("LOG_LEVEL") {
    Ok(level) => level,
    Err(_) => "info".to_string(),
  };

  SimpleLogger::new()
    .with_level(level.parse().unwrap())
    .with_module_level("biab_utils", level.parse().unwrap())
    .init()
    .unwrap();
}
