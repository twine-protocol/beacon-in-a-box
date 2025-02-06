use chrono::TimeDelta;
use std::sync::Arc;
use twine::prelude::*;
use twine::twine_core::multihash_codetable::Code;
use twine::twine_core::multihash_codetable::Multihash;
use twine::twine_core::verify::{Verifiable, Verified};
use twine::twine_core::Bytes;

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
    // verify that the timestamp doesn't have any ms
    if self.timestamp.timestamp_subsec_millis() != 0 {
      return Err(VerificationError::Payload(
        "Timestamp has milliseconds".to_string(),
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

  pub fn from_rand(
    rand: Vec<u8>,
    pre: Multihash,
    prev: Arc<Tixel>,
    period: chrono::TimeDelta,
  ) -> Result<Self, VerificationError> {
    // ensure rand corresponds to previous pre
    let prev_payload = prev.extract_payload::<RandomnessPayload>()?;

    let code = Code::try_from(prev_payload.0.pre.code())
      .map_err(|_| VerificationError::UnsupportedHashAlgorithm)?;

    use twine::twine_core::multihash_codetable::MultihashDigest;
    if prev_payload.0.pre != code.digest(&rand) {
      return Err(VerificationError::Payload(
        "Precommitment does not match random bytes".to_string(),
      ));
    }

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
    let timestamp =
      crate::timing::next_pulse_timestamp(prev_payload.0.timestamp, period);
    Self::try_new(salt, pre, timestamp)
  }

  pub fn new_start(
    pre: Multihash,
    period: TimeDelta,
  ) -> Result<Self, VerificationError> {
    let num_bytes = pre.size();
    let salt = Bytes((0..num_bytes).collect());
    let timestamp = crate::timing::next_truncated_time(period);
    Self::try_new(salt, pre, timestamp)
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

  pub fn timestamp(&self) -> chrono::DateTime<chrono::Utc> {
    self.0.timestamp
  }

  pub fn salt(&self) -> &[u8] {
    &self.0.salt.0
  }

  pub fn pre(&self) -> Multihash {
    self.0.pre
  }
}
