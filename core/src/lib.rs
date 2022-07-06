#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(not(any(feature = "network-smoltcp", feature = "network-native")))]
compile_error!("You must enable at one network feature");

#[cfg(all(feature = "network-smoltcp", feature = "network-native"))]
compile_error!("You must select at least one network feature");

#[macro_use]
extern crate log;

mod leader_election;

#[cfg(feature = "network-native")]
pub mod socket_native;

#[cfg(feature = "network-smoltcp")]
pub mod socket_smoltcp;

use heapless::{String, Vec};
use leader_election::LeaderElection;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use zerocopy::{AsBytes, FromBytes};

const CHANNELS: usize = 8;
const BLOCK_SIZE: usize = 48;
type SampleType = i16;

#[cfg(feature = "network-native")]
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

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
pub enum PatchState {
    Idle,
    PatchEnabled,
    PatchToggled,
    Blocked,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct HeldInputJack {
    uuid: Uuid,
    id: JackId,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct HeldOutputJack {
    uuid: Uuid,
    id: JackId,
    color: u32,
    addr: [u8; 4],
    // port: u16,
}

#[derive(PartialEq, Serialize, Deserialize, Default, Clone, Debug)]
struct LocalState {
    held_inputs: Vec<HeldInputJack, JW>,
    held_outputs: Vec<HeldOutputJack, JW>,
    // Not sure why this fails with a lifetime error without the following line, but otherwise
    // everything parses correctly...
    // make_compile: Option<bool>,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct PatchConnection {
    input_uuid: Uuid,
    input_jack_id: JackId,
    output_uuid: Uuid,
    output_jack_id: JackId,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveSetInputJack {
    uuid: Uuid,
    source: HeldOutputJack,
    connection: PatchConnection,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveSetOutputJack {
    uuid: Uuid,
    source: HeldInputJack,
    connection: PatchConnection,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveHalt {
    uuid: Uuid,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveHeartbeat {
    uuid: Uuid,
    term: u32,
    iteration: u32,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveHeartbeatResponse {
    uuid: Uuid,
    term: u32,
    success: bool,
    iteration: Option<u32>,
    state: Option<LocalState>,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveRequestVote {
    uuid: Uuid,
    term: u32,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveRequestVoteResponse {
    uuid: Uuid,
    term: u32,
    voted_for: Uuid,
    vote_granted: bool,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
struct DirectiveGlobalStateUpdate {
    uuid: Uuid,
    patch_state: PatchState,
    input: Option<HeldInputJack>,
    output: Option<HeldOutputJack>,
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Debug)]
enum Directive {
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

/// General backend communication control.
///
/// Since the backend networking can be changed to run on a host operating system or on a full
/// network stack, this trait defines what methods are needed to be implemented to accomplish this.
pub trait Network {
    /// Update internal state and send/recv packets, if needed
    fn poll(&mut self, _time: i64) -> Result<bool, Error> {
        Ok(true)
    }
    /// Check if socket is ready for sending
    fn can_send(&mut self) -> bool;
    /// Get bytes from the directive multicast
    fn recv_directive(&mut self, buf: &mut [u8]) -> Result<usize, Error>;
    /// Output bytes on the directive multicast
    fn send_directive(&mut self, buf: &[u8]) -> Result<(), Error>;
    /// Connect an input jack to an output endpoint
    fn jack_connect(&mut self, jack_id: usize, addr: &str, time: i64) -> Result<(), Error>;
    /// Get audio data for a particular jack
    fn jack_recv(&mut self, jack_id: usize, buf: &mut [u8]) -> Result<usize, Error>;
    /// Send audio data for a particular jack
    fn jack_send(&mut self, jack_id: usize, buf: &[u8]) -> Result<(), Error>;
    /// Get multicast address for a particular jack
    fn jack_addr(&mut self, jack_id: usize) -> Result<[u8; 4], Error>;
}

/// Module communication and state handling.
///
/// `Module` is responsible for handling communication between other modules on the same network on
/// within the same process (depending on configuration), as well as manages the current state of
/// patching and the audio packet reception and tranmission.
///
/// Since this portion is platform independent, with `no-std` and no allocation, users of this crate
/// are responsible for providing the current time (in milliseconds from an arbitrary start), a
/// source of random source, and `poll`-ing the module at regular intervals to perform network
/// updates.
pub struct Module<T: Network, R: RngCore> {
    uuid: Uuid,
    interface: T,
    leader_election: LeaderElection<R>,
    input_count: usize,
    output_count: usize,
    input_patch_enabled: u16,
    output_patch_enabled: u16,
}

impl<T: Network, R: RngCore> Module<T, R> {
    pub fn new(interface: T, rand_source: R, id: Uuid, time: i64, input_count: usize, output_count: usize) -> Self {
        let leader_election = LeaderElection::new(id.clone(), time, rand_source);
        Module {
            uuid: id,
            interface,
            leader_election,
            input_count,
            output_count,
            input_patch_enabled: 0,
            output_patch_enabled: 0,
        }
    }

    pub fn poll(&mut self, time: i64) -> Result<(), Error> {
        self.interface.poll(time)?;
        if self.can_send() {
            while let Ok(d) = self.recv_directive() {
                if let Some(resp) = self.leader_election.poll(Some(d), time) {
                    self.send_directive(&resp)?;
                }
            }
            if let Some(resp) = self.leader_election.poll(None, time) {
                self.send_directive(&resp)?;
            }
        } else {
            self.leader_election.reset(time);
        }
        Ok(())
    }

    pub fn can_send(&mut self) -> bool {
        self.interface.can_send()
    }

    fn recv_directive(&mut self) -> Result<Directive, Error> {
        let mut buf = [0; 2048];
        match self.interface.recv_directive(&mut buf) {
            Ok(size) => match serde_json_core::from_slice(&buf[0..size]) {
                Ok((out, _)) => {
                    trace!("<= {:?}", out);
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

    fn send_directive(&mut self, directive: &Directive) -> Result<(), Error> {
        trace!("=> {:?}", directive);
        let mut buf = [0; 2048];
        match serde_json_core::to_slice(directive, &mut buf) {
            Ok(len) => self.interface.send_directive(&buf[0..len]),
            Err(_) => Err(Error::Parse),
        }
    }

    pub fn jack_connect(&mut self, jack_id: usize, addr: &str, time: i64) -> Result<(), Error> {
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

    pub fn send_halt(&mut self) {
        let out = Directive::Halt(DirectiveHalt {
            uuid: "GLOBAL".into(),
        });
        if let Err(e) = self.send_directive(&out) {
            info!("Halt command failed {:?}", e);
        }
    }

    pub fn set_input_patch_enabled(&mut self, jack_id: usize, status: bool) -> Result<(), Error> {
        if jack_id >= self.input_count {
            Err(Error::InvalidJackId)
        } else {
            if status {
                self.input_patch_enabled |= 1 << jack_id;
            } else {
                self.input_patch_enabled &= !(1 << jack_id);
            }
            self.update_patch_state()
        }
    }

    pub fn set_output_patch_enabled(&mut self, jack_id: usize, status: bool) -> Result<(), Error> {
        if jack_id >= self.output_count {
            Err(Error::InvalidJackId)
        } else {
            if status {
                self.output_patch_enabled |= 1 << jack_id;
            } else {
                self.output_patch_enabled &= !(1 << jack_id);
            }
            self.update_patch_state()
        }
    }

    fn update_patch_state(&mut self) -> Result<(), Error> {
        let mut local_state: LocalState = Default::default();
        for i in 0..self.input_count {
            if (self.input_patch_enabled & (1 << i)) != 0 {
                local_state.held_inputs.push(HeldInputJack {
                    uuid: self.uuid.clone(),
                    id: i as u32,
                }).unwrap();
            }
        }
        for i in 0..self.output_count {
            if (self.output_patch_enabled & (1 << i)) != 0 {
                local_state.held_outputs.push(HeldOutputJack { 
                    uuid: self.uuid.clone(),
                    id: i as u32,
                    color: 30,
                    addr: self.interface.jack_addr(i)?,
                }).unwrap();
            }
        }
        self.leader_election.update_local_state(local_state);
        Ok(())
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
