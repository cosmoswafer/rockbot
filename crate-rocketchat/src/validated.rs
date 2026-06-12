use nutype::nutype;

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct ServerUrl(String);

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct Username(String);

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct Password(String);
