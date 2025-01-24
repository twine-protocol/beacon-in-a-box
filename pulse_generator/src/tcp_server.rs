use std::{env, net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::Notify};

// TCP Server to listen for messages
pub async fn start_tcp_server(shutdown: Arc<Notify>) {
  // Load environment variables
  let addr: String = env::var("LISTEN_ADDR").unwrap_or_else(|_| "0.0.0.0:5555".to_string());

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

      if message.command == "randomness" {
        if let Ok(Some(randomness)) = message.extract_payload::<Vec<u8>>() {
          log::info!(
            "[{}] Received {} bits of randomness",
            peer,
            randomness.len() * 8
          );
        }
      }
    }
  }

  // log::debug!("[{}] Connection closed", peer);
}
