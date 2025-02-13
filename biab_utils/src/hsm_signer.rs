use twine::prelude::*;
use twine::twine_core::crypto::Signature;
use twine::{twine_builder::Signer, twine_core::crypto::PublicKey};
use yubihsm::object::Type;
use yubihsm::{rsa::Algorithm, Client};

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
  let public_key = public_key.as_ref();
  let modulus = public_key.len() * 8;
  let info = client.get_object_info(key_id, Type::AsymmetricKey)?;
  // for now only support RSA
  let alg = info
    .algorithm
    .rsa()
    .ok_or(anyhow::anyhow!("Only RSA supported"))?;
  let signing_alg = match alg {
    Algorithm::Pkcs1(sha) => match sha {
      yubihsm::rsa::pkcs1::Algorithm::Sha256 => {
        twine::twine_core::crypto::SignatureAlgorithm::Sha256Rsa(modulus)
      }
      // yubihsm::rsa::pkcs1::Algorithm::Sha384 => {
      //   twine::twine_core::crypto::SignatureAlgorithm::Sha384Rsa(modulus)
      // }
      // yubihsm::rsa::pkcs1::Algorithm::Sha512 => {
      //   twine::twine_core::crypto::SignatureAlgorithm::Sha512Rsa(modulus)
      // }
      _ => {
        return Err(anyhow::anyhow!("Unsupported RSA signing algorithm"));
      }
    },
    _ => {
      return Err(anyhow::anyhow!("Unsupported key type"));
    }
  };

  Ok(PublicKey::new(signing_alg, public_key.into()))
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
