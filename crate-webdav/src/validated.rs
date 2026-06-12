use nutype::nutype;

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct DavUrl(String);

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct DavUsername(String);

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct DavPassword(String);

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct DavRoot(String);

#[nutype(validate(not_empty), derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Deref))]
pub struct DavPath(String);
