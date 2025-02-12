use serde::{Deserialize, Serialize, Serializer};

pub fn serialize<S: Serializer, T: Serialize>(
  value: &T,
  serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
  let ser = twine::twine_core::serde_ipld_dagjson::Serializer::new(serializer);
  value.serialize(ser)
}

pub fn deserialize<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
  deserializer: D,
) -> std::result::Result<T, D::Error> {
  let de =
    twine::twine_core::serde_ipld_dagjson::Deserializer::new(deserializer);
  Deserialize::deserialize(de)
}
