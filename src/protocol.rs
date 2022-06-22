use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub enum Directive<'a> {
    Halt { uuid: &'a str },
}
