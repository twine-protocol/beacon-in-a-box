use rsa::pkcs1::EncodeRsaPublicKey;
use twine_protocol::prelude::*;
use twine_protocol::twine_lib::crypto::Signature;
use twine_protocol::{twine_builder::Signer, twine_lib::crypto::PublicKey};
use yubihsm::object::Type;
use yubihsm::{asymmetric::Algorithm, Client};

pub struct HsmSigner {
  client: Client,
  public_key: PublicKey,
  key_id: u16,
}

fn get_public_key(
  client: &Client,
  key_id: u16,
) -> Result<PublicKey, anyhow::Error> {
  let public_key = client.get_public_key(key_id)?;
  let n = public_key.as_ref();
  let info = client.get_object_info(key_id, Type::AsymmetricKey)?;
  // for now only support RSA
  let alg = info.algorithm.asymmetric().ok_or(anyhow::anyhow!(
    "Only Asymmetric RSA supported. Found: {:?}",
    info.algorithm
  ))?;
  let signing_alg = match alg {
    Algorithm::Rsa2048 => {
      twine_protocol::twine_lib::crypto::SignatureAlgorithm::Sha256Rsa(2048)
    }
    _ => {
      return Err(anyhow::anyhow!("Unsupported key type. Found: {:?}", alg));
    }
  };

  let n = rsa::BigUint::from_bytes_be(n);
  let e = rsa::BigUint::from_bytes_be(&vec![0x01, 0x00, 0x01]);
  let asn1der = rsa::RsaPublicKey::new(n, e)?
    .to_pkcs1_der()
    .map_err(|e| anyhow::anyhow!("Failed to encode public key: {}", e))?;

  Ok(PublicKey::new(signing_alg, asn1der.as_bytes().into()))
}

impl HsmSigner {
  pub fn try_new(client: Client, key_id: u16) -> Result<Self, anyhow::Error> {
    let public_key = get_public_key(&client, key_id)?;
    Ok(HsmSigner {
      client,
      public_key,
      key_id,
    })
  }
}

impl Signer for HsmSigner {
  type Key = PublicKey;

  fn public_key(&self) -> Self::Key {
    self.public_key.clone()
  }

  fn sign<T: AsRef<[u8]>>(&self, data: T) -> Result<Signature, SigningError> {
    let sig = self
      .client
      .sign_rsa_pkcs1v15_sha256(self.key_id, data.as_ref())
      .map_err(|e| SigningError(e.to_string()))?;

    Ok(sig.as_ref().into())
  }
}
