#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(not(any(feature = "network-smoltcp", feature = "network-native")))]
compile_error!("You must enable at one network feature");

#[cfg(all(feature = "network-smoltcp", feature = "network-native"))]
compile_error!("You must select at least one network feature");

#[macro_use]
extern crate log;

pub mod leader_election;

#[cfg(feature = "network-native")]
pub mod socket_native;

#[cfg(feature = "network-smoltcp")]
pub mod socket_smoltcp;

use heapless::{String, Vec};
use serde::{Deserialize, Serialize};
use zerocopy::{AsBytes, FromBytes};

const CHANNELS: usize = 8;
const BLOCK_SIZE: usize = 48;
type SampleType = i16;

const PREFERRED_SUBNET: &str = "10.0.0.0/8";
const PATCH_EP: &str = "239.0.0.0:19874";
const JACK_PORT: u16 = 19991;

const SW: usize = 48;
const JW: usize = 15;
pub type Uuid = String<SW>;
type JackId = u32;

#[derive(AsBytes, FromBytes, Copy, Clone, Default, Debug)]
#[repr(C)]
pub struct AudioFrame {
    pub data: [SampleType; CHANNELS],
}

#[derive(AsBytes, FromBytes, Debug)]
#[repr(C)]
pub struct AudioPacket {
    pub data: [AudioFrame; BLOCK_SIZE],
}

impl Default for AudioPacket {
    fn default() -> Self {
        AudioPacket {
            data: [Default::default(); BLOCK_SIZE],
        }
    }
}

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

#[derive(Debug)]
pub enum Error {
    General,
    Network,
    NoData,
    InvalidJackId,
    Parse,
}

pub trait Network {
    // Update internal state and send/recv packets, if needed
    fn poll(&mut self, _time: i64) -> Result<bool, Error> {
        Ok(true)
    }
    // Check if socket is ready for sending
    fn can_send(&mut self) -> bool;
    // Get bytes from the directive multicast
    fn recv_directive(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
    // Output bytes on the directive multicast
    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error>;
    // Connect an input jack to an output endpoint
    fn jack_connect(&mut self, jack_id: usize, addr: &str, time: i64)
        -> Result<(), Error>;
    // Get audio data for a particular jack
    fn jack_recv(&mut self, jack_id: usize, buf: &mut [u8]) -> Result<usize, Error>;
    // Send audio data for a particular jack
    fn jack_send(&mut self, jack_id: usize, buf: &[u8]) -> Result<(), Error>;
}

pub struct Module<T: Network> {
    interface: T,
}

impl<T: Network> Module<T> {
    pub fn new(interface: T) -> Self {
        Module { interface }
    }

    pub fn poll(&mut self, time: i64) -> Result<bool, Error> {
        self.interface.poll(time)?;
        loop {
            match self.recv_directive() {
                Ok(_) => {}
                Err(_) => break,
            }
        }
        Ok(false)
    }

    pub fn can_send(&mut self) -> bool {
        self.interface.can_send()
    }

    pub fn recv_directive(&mut self) -> Result<Directive, Error> {
        let mut buf = [0; 2048];
        match self.interface.recv_directive(&mut buf) {
            Ok(size) => match serde_json_core::from_slice(&mut buf[0..size]) {
                Ok((out, _)) => {
                    info!("<= {:?}", out);
                    Ok(out)
                }
                Err(e) => {
                    info!("JSON Parse Error: {:?}", e);
                    Err(Error::Parse)
                }
            },
            Err(_) => Err(Error::NoData),
        }
    }

    pub fn send_directive(&mut self, directive: &Directive) -> Result<(), Error> {
        info!("=> {:?}", directive);
        let mut buf = [0; 2048];
        match serde_json_core::to_slice(directive, &mut buf) {
            Ok(len) => self.interface.send_directive(&buf[0..len]),
            Err(_) => Err(Error::Parse),
        }
    }

    pub fn jack_connect(
        &mut self,
        jack_id: usize,
        addr: &str,
        time: i64,
    ) -> Result<(), Error> {
        self.interface.jack_connect(jack_id, addr, time)
    }

    pub fn jack_recv(&mut self, jack_id: usize) -> Result<AudioPacket, Error> {
        let mut buf = [0; 2048];
        let size = self.interface.jack_recv(jack_id, &mut buf)?;
        match AudioPacket::read_from(&mut buf[0..size]) {
            Some(res) => Ok(res),
            None => Err(Error::Parse),
        }
    }

    pub fn jack_send(&mut self, jack_id: usize, data: &AudioPacket) -> Result<(), Error> {
        self.interface.jack_send(jack_id, data.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
