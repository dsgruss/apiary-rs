use heapless::Vec;
use serde::{Deserialize, Serialize};

pub enum PatchState {
    Idle,
    PatchEnabled,
    PatchToggled,
    Blocked,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HeldInputJack<'a> {
    uuid: &'a str,
    id: &'a str,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HeldOutputJack<'a> {
    uuid: &'a str,
    id: &'a str,
    color: u32,
    addr: &'a str,
    port: u16,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LocalState<'a> {
    held_inputs: Vec<HeldInputJack<'a>, 15>,
    held_outputs: Vec<HeldOutputJack<'a>, 15>,
    // Not sure why this fails with a lifetime error without the following line, but otherwise
    // everything parses correctly...
    make_compile: Option<&'a str>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Directive<'a> {
    Update {
        uuid: &'a str,
        local_state: LocalState<'a>,
    },
    Halt {
        uuid: &'a str,
    },
}
