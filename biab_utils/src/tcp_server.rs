use crate::{Message, Messenger};
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::TcpListener, sync::Notify};

// TCP Server to listen for messages
pub fn start_tcp_server(
  addr: String,
  shutdown: Arc<Notify>,
) -> tokio::sync::mpsc::Receiver<Message> {
  let (tx, rx) = tokio::sync::mpsc::channel(32);

  let messenger = Messenger::new();

  tokio::spawn(async move {
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
        _ = shutdown.notified() => {
          log::debug!("Shutting down TCP server...");
          break;
        }
        result = listener.accept() => {
          match result {
            Ok((stream, peer)) => {
              log::debug!("New connection from {}", peer);
              tokio::spawn(handle_client(messenger.clone(), stream, peer, tx.clone()));
            }
            Err(e) => {
              log::error!("Failed to accept connection: {}", e);
            }
          }
        }
      }
    }
  });

  rx
}

async fn handle_client(
  messenger: Messenger,
  mut stream: tokio::net::TcpStream,
  peer: SocketAddr,
  tx: tokio::sync::mpsc::Sender<Message>,
) {
  loop {
    if let Some(message) = messenger.receive(&mut stream).await {
      log::debug!("[{}] Received message: {:?}", peer, message);

      if let Err(e) = tx.send(message).await {
        log::error!("[{}] Failed to broadcast recieved message: {}", peer, e);
      }
    }
  }
}
