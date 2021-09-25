use serde::{Deserialize, Serialize};

pub mod keybase;

#[derive(Deserialize, Serialize, Debug)]
pub enum Token {
    Keybase(keybase::Token),
}
