use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Token {
    pub realm_access: RealmAccess,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct RealmAccess {
    pub roles: Vec<String>,
}
