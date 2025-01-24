use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::sync::Arc;
use std::sync::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub struct Message {
  pub id: uuid::Uuid,
  pub timestamp: chrono::DateTime<chrono::Utc>,
  pub command: String,
  pub payload: Option<Vec<u8>>,
}

impl Message {
  pub fn extract_payload<T: DeserializeOwned>(&self) -> bincode::Result<Option<T>> {
    self
      .payload
      .as_ref()
      .map(|p| bincode::deserialize(p))
      .transpose()
  }
}

impl AsRef<Message> for Message {
  fn as_ref(&self) -> &Message {
    self
  }
}

#[derive(Debug, Clone)]
pub struct Messenger {
  latest: Arc<RwLock<Option<Message>>>,
}

impl Messenger {
  pub fn new() -> Self {
    Self {
      latest: Arc::new(RwLock::new(None)),
    }
  }

  pub fn latest(&self) -> Option<Message> {
    self.latest.read().expect("Failed to acquire lock").clone()
  }

  pub fn text(&self, command: &str) -> Message {
    self.prepare(command, None::<&()>)
  }

  pub fn delivery<T: Serialize>(&self, command: &str, payload: &T) -> Message {
    self.prepare(command, Some(payload))
  }

  fn prepare<T: Serialize>(&self, command: &str, payload: Option<&T>) -> Message {
    let id = uuid::Uuid::new_v4();
    let timestamp = chrono::Utc::now();
    let payload = payload.map(|p| bincode::serialize(p).expect("Failed to serialize payload"));
    let message = Message {
      id,
      timestamp,
      command: command.to_string(),
      payload,
    };
    message
  }

  pub async fn send_text(&self, stream: &mut TcpStream, command: &str) -> tokio::io::Result<()> {
    self.send(stream, self.text(command)).await
  }

  pub async fn send_delivery<T: Serialize>(
    &self,
    stream: &mut TcpStream,
    command: &str,
    payload: &T,
  ) -> tokio::io::Result<()> {
    self.send(stream, self.delivery(command, payload)).await
  }

  /// Asynchronously send a message over a TCP stream
  async fn send<M: AsRef<Message>>(
    &self,
    stream: &mut TcpStream,
    message: M,
  ) -> tokio::io::Result<()> {
    let serialized = bincode::serialize(message.as_ref()).expect("Failed to serialize message");
    let len = serialized.len() as u32;

    let mut writer = BufWriter::new(stream);
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&serialized).await?;
    writer.flush().await?;
    Ok(())
  }

  /// Asynchronously receive a message from a TCP stream
  pub async fn receive(&self, stream: &mut TcpStream) -> Option<Message> {
    let mut reader = BufReader::new(stream);

    let mut len_buf = [0; 4];
    reader.read_exact(&mut len_buf).await.ok()?;
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut data_buf = vec![0; len];
    reader.read_exact(&mut data_buf).await.ok()?;

    let message: Message = match bincode::deserialize(&data_buf) {
      Ok(message) => message,
      Err(e) => {
        log::error!("Failed to deserialize message: {}", e);
        return None;
      }
    };

    match &*self.latest.read().expect("Failed to acquire lock") {
      Some(latest) => {
        if message.timestamp < latest.timestamp {
          log::warn!("Received message with older timestamp");
          return None;
        }
        if message.id == latest.id {
          log::warn!("Received duplicate message. Ignoring.");
          return None;
        }
      }
      None => {}
    };

    // Store the latest received message
    *self.latest.write().expect("Failed to acquire lock") = Some(message.clone());

    Some(message)
  }

  // pub fn receive<B: AsRef<[u8]>>(&self, bytes: B) -> Option<Message> {
  //   let message: Message = match bincode::deserialize(bytes.as_ref()) {
  //     Ok(message) => message,
  //     Err(e) => {
  //       log::error!("Failed to deserialize message: {}", e);
  //       return None;
  //     }
  //   };

  //   let mut latest = self.latest.write().expect("Failed to acquire lock");

  //   // check timestamp and id
  //   match &*latest {
  //     Some(latest) => {
  //       if message.timestamp < latest.timestamp {
  //         log::warn!("Received message with older timestamp");
  //         return None;
  //       }
  //       if message.id == latest.id {
  //         log::warn!("Received duplicate message. Ignoring.");
  //         return None;
  //       }
  //     }
  //     None => {}
  //   };

  //   *latest = Some(message.clone());
  //   Some(message)
  // }
}
