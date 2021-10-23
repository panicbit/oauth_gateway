use serde::{Deserialize, Serialize};

pub mod keybase;

#[derive(Deserialize, Serialize, Debug)]
#[serde(untagged)]
pub enum Token {
    Keybase(keybase::Token),
}
