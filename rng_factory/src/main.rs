use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;

#[tokio::main]
async fn main() -> std::io::Result<()> {
  let mut stream = TcpStream::connect("generator:5555").await?;
  stream.write_all(b"Hello from client").await?;
  Ok(())
}
