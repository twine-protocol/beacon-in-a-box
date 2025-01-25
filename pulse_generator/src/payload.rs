use chrono::Timelike;
use std::sync::Arc;
use twine::prelude::*;
use twine::twine_core::multihash_codetable::Code;
use twine::twine_core::multihash_codetable::Multihash;
use twine::twine_core::verify::{Verifiable, Verified};
use twine::twine_core::Bytes;

fn top_of_the_minute() -> chrono::DateTime<chrono::Utc> {
  let now = chrono::Utc::now();
  let top_of_the_minute =
    now.with_second(0).unwrap().with_nanosecond(0).unwrap();

  top_of_the_minute + chrono::Duration::minutes(1)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RandomnessPayloadRaw {
  salt: Bytes,
  pre: Multihash,
  timestamp: chrono::DateTime<chrono::Utc>,
}

impl Verifiable for RandomnessPayloadRaw {
  fn verify(&self) -> Result<(), VerificationError> {
    if self.salt.len() != self.pre.size() as usize {
      return Err(VerificationError::Payload(
        "Salt length does not match pre hash size".to_string(),
      ));
    }
    Ok(())
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RandomnessPayload(Verified<RandomnessPayloadRaw>);

impl RandomnessPayload {
  pub fn try_new(
    salt: Bytes,
    pre: Multihash,
    timestamp: chrono::DateTime<chrono::Utc>,
  ) -> Result<Self, VerificationError> {
    Verified::try_new(RandomnessPayloadRaw {
      salt,
      pre,
      timestamp,
    })
    .map(Self)
  }

  pub fn try_new_now(
    salt: Bytes,
    pre: Multihash,
  ) -> Result<Self, VerificationError> {
    let timestamp = top_of_the_minute();
    Self::try_new(salt, pre, timestamp)
  }

  pub fn from_rand(
    rand: Vec<u8>,
    pre: Multihash,
    prev: Arc<Tixel>,
  ) -> Result<Self, VerificationError> {
    if prev.cid().hash().size() != pre.size() {
      return Err(VerificationError::Payload(
        "Pre hash size does not match previous tixel hash size".to_string(),
      ));
    }
    // we xor the random bytes with previous cid hash digest
    let salt = Bytes(
      rand
        .iter()
        .zip(prev.cid().hash().digest().iter())
        .map(|(a, b)| a ^ b)
        .collect(),
    );
    Self::try_new_now(salt, pre)
  }

  pub fn new_start(pre: Multihash) -> Result<Self, VerificationError> {
    let num_bytes = pre.size();
    let salt = Bytes((0..num_bytes).collect());
    Self::try_new_now(salt, pre)
  }

  pub fn validate_randomness(
    &self,
    prev: Arc<Tixel>,
  ) -> Result<(), VerificationError> {
    if prev.cid().hash().size() != self.0.pre.size() {
      return Err(VerificationError::Payload(
        "Pre hash size does not match previous tixel hash size".to_string(),
      ));
    }
    let prev_payload = prev.extract_payload::<RandomnessPayload>()?;
    if self.0.timestamp < prev_payload.0.timestamp {
      return Err(VerificationError::Payload(
        "Timestamp is less than previous tixel timestamp".to_string(),
      ));
    }
    // check that the precommitment from the previous tixel matches the xor rand value
    let rand = self
      .0
      .salt
      .iter()
      .zip(prev.cid().hash().digest().iter())
      .map(|(a, b)| a ^ b)
      .collect::<Vec<u8>>();

    use twine::twine_core::multihash_codetable::MultihashDigest;
    let code = Code::try_from(prev_payload.0.pre.code())
      .map_err(|_| VerificationError::UnsupportedHashAlgorithm)?;
    let pre = code.digest(&rand);
    if pre != self.0.pre {
      return Err(VerificationError::Payload(
        "Pre hash does not match previous tixel pre hash".to_string(),
      ));
    }
    Ok(())
  }

  pub fn extract_randomness(
    current: Arc<Tixel>,
    prev: Arc<Tixel>,
  ) -> Result<Vec<u8>, VerificationError> {
    let payload = current.extract_payload::<RandomnessPayload>()?;
    if let Err(e) = payload.validate_randomness(prev) {
      return Err(e);
    }
    Ok(current.cid().hash().digest().to_vec())
  }
}
