use nutype::nutype;

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref)
)]
pub struct NonEmptyString(String);

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref)
)]
pub struct ProviderName(String);

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref)
)]
pub struct ModelAlias(String);

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref)
)]
pub struct ApiKey(String);

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref)
)]
pub struct ConfigUrl(String);

#[nutype(
    validate(not_empty),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref)
)]
pub struct ConfigUsername(String);

#[nutype(
    validate(greater_or_equal = 1, less_or_equal = 100_000_000),
    derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Deref, Copy)
)]
pub struct BoundedUsize(usize);
