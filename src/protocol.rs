use heapless::{String, Vec};
use serde::{Deserialize, Serialize};

const SW: usize = 48;
const JW: usize = 15;
pub type Uuid = String<SW>;
type JackId = u32;

#[derive(Serialize, Deserialize, Debug)]
pub enum PatchState {
    Idle,
    PatchEnabled,
    PatchToggled,
    Blocked,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HeldInputJack {
    pub uuid: Uuid,
    pub id: JackId,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HeldOutputJack {
    pub uuid: Uuid,
    pub id: JackId,
    pub color: u32,
    pub addr: String<SW>,
    pub port: u16,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct LocalState {
    pub held_inputs: Vec<HeldInputJack, JW>,
    pub held_outputs: Vec<HeldOutputJack, JW>,
    // Not sure why this fails with a lifetime error without the following line, but otherwise
    // everything parses correctly...
    // make_compile: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PatchConnection {
    input_uuid: Uuid,
    input_jack_id: JackId,
    output_uuid: Uuid,
    output_jack_id: JackId,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Directive {
    SetInputJack {
        uuid: Uuid,
        source: HeldOutputJack,
        connection: PatchConnection,
    },
    SetOutputJack {
        uuid: Uuid,
        source: HeldInputJack,
        connection: PatchConnection,
    },
    Update {
        uuid: Uuid,
        local_state: LocalState,
    },
    Halt {
        uuid: Uuid,
    },
    Heartbeat {
        uuid: Uuid,
        term: u32,
        iteration: u32,
    },
    HeartbeatResponse {
        uuid: Uuid,
        term: u32,
        success: bool,
        iteration: Option<u32>,
        state: Option<LocalState>,
    },
    RequestVote {
        uuid: Uuid,
        term: u32,
    },
    RequestVoteResponse {
        uuid: Uuid,
        term: u32,
        voted_for: Uuid,
        vote_granted: bool,
    },
    GlobalStateUpdate {
        uuid: Uuid,
        patch_state: PatchState,
        input: Option<HeldInputJack>,
        output: Option<HeldOutputJack>,
    }
}
