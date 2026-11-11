use basso_buf::AzBuf;
use basso_registry::identifier::Identifier;

#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[derive(AzBuf, Clone, Debug, simdnbt::Deserialize, PartialEq)]
pub struct CustomSound {
    pub sound_id: Identifier,
    pub range: Option<f32>,
}
