use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::signal::unix::{signal, SignalKind};

#[tokio::main]
async fn main() -> std::io::Result<()> {
  let listener = TcpListener::bind("0.0.0.0:5555").await?;
  let mut sigint = tokio::signal::ctrl_c();
  let mut sigterm = signal(SignalKind::terminate())?;

  tokio::select! {
    _ = async {
      while let Ok((mut socket, _)) = listener.accept().await {
        let mut buf = vec![0; 1024];
        let n = socket.read(&mut buf).await?;
        println!("Received: {}", String::from_utf8_lossy(&buf[..n]));
      }
      Ok::<_, tokio::io::Error>(())
    } => {},
    _ = sigint => {
      println!("Received Ctrl-C, shutting down.");
    },
    _ = sigterm.recv() => {
      println!("Received SIGTERM, shutting down.");
    },
  }

  Ok(())
}
