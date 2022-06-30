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
pub struct DirectiveSetInputJack {
    pub uuid: Uuid,
    pub source: HeldOutputJack,
    pub connection: PatchConnection,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveSetOutputJack {
    pub uuid: Uuid,
    pub source: HeldInputJack,
    pub connection: PatchConnection,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveHalt {
    pub uuid: Uuid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveHeartbeat {
    pub uuid: Uuid,
    pub term: u32,
    pub iteration: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveHeartbeatResponse {
    pub uuid: Uuid,
    pub term: u32,
    pub success: bool,
    pub iteration: Option<u32>,
    pub state: Option<LocalState>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveRequestVote {
    pub uuid: Uuid,
    pub term: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveRequestVoteResponse {
    pub uuid: Uuid,
    pub term: u32,
    pub voted_for: Uuid,
    pub vote_granted: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DirectiveGlobalStateUpdate {
    pub uuid: Uuid,
    pub patch_state: PatchState,
    pub input: Option<HeldInputJack>,
    pub output: Option<HeldOutputJack>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Directive {
    SetInputJack(DirectiveSetInputJack),
    SetOutputJack(DirectiveSetOutputJack),
    Halt(DirectiveHalt),
    Heartbeat(DirectiveHeartbeat),
    HeartbeatResponse(DirectiveHeartbeatResponse),
    RequestVote(DirectiveRequestVote),
    RequestVoteResponse(DirectiveRequestVoteResponse),
    GlobalStateUpdate(DirectiveGlobalStateUpdate),
}
