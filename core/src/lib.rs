#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(not(any(
    feature = "network-smoltcp",
    feature = "network-native",
    feature = "network-local"
)))]
compile_error!("You must enable exactly one network feature");

#[cfg(all(
    feature = "network-smoltcp",
    feature = "network-native",
    feature = "network-local"
))]
compile_error!("You must enable exactly one network feature");

#[macro_use]
extern crate log;

mod leader_election;

#[cfg(feature = "network-native")]
pub mod socket_native;

#[cfg(feature = "network-smoltcp")]
pub mod socket_smoltcp;

#[cfg(feature = "network-local")]
pub mod socket_local;

#[cfg(feature = "network-local")]
#[macro_use]
extern crate lazy_static;

use heapless::String;
use leader_election::LeaderElection;
use palette::{Hsv, IntoColor, Srgb};
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use zerocopy::{AsBytes, FromBytes};

pub const CHANNELS: usize = 8;
pub const BLOCK_SIZE: usize = 48;
pub type SampleType = i16;

#[cfg(feature = "network-native")]
const PREFERRED_SUBNET: &str = "10.0.0.0/8";

const PATCH_EP: &str = "239.0.0.0:19874";
const JACK_PORT: u16 = 19991;

pub const SAMPLE_RATE: f32 = 48000.0;

pub fn midi_note_to_voct(note: u8) -> i16 {
    (note as i16 - 64) * 512
}

pub fn voct_to_frequency(v_oct: i16) -> f32 {
    440.0 * voct_to_freq_scale(v_oct as f32 - 5.0)
}

#[cfg(feature = "std")]
pub fn voct_to_freq_scale(v_oct: f32) -> f32 {
    2.0_f32.powf((v_oct) / (512.0 * 12.0))
}

#[cfg(not(feature = "std"))]
pub fn voct_to_freq_scale(v_oct: f32) -> f32 {
    use libm::powf;
    powf(2.0, (v_oct) / (512.0 * 12.0))
}

pub fn softclip(x: f32) -> f32 {
    let y = if x < -3.0 {
        -3.0
    } else if x > 3.0 {
        3.0
    } else {
        x
    };
    y * (27.0 + y * y) / (27.0 + 9.0 * y * y)
}

const SW: usize = 48;
pub type Uuid = String<SW>;
type JackId = u32;

#[derive(AsBytes, FromBytes, Copy, Clone, Default, Debug)]
#[repr(C)]
pub struct AudioFrame {
    pub data: [SampleType; CHANNELS],
}

#[derive(AsBytes, FromBytes, Copy, Clone, Debug)]
#[repr(C)]
pub struct AudioPacket {
    pub data: [AudioFrame; BLOCK_SIZE],
}

impl AudioPacket {
    pub fn avg(&self) -> f32 {
        self.data
            .iter()
            .map(|x| x.data.iter().map(|y| *y as f32).sum::<f32>())
            .sum::<f32>()
            / (BLOCK_SIZE as f32 * CHANNELS as f32)
    }
}

impl Default for AudioPacket {
    fn default() -> Self {
        AudioPacket {
            data: [Default::default(); BLOCK_SIZE],
        }
    }
}

#[derive(PartialEq, Serialize, Deserialize, Copy, Clone, Debug)]
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
    color: u16,
    addr: [u8; 4],
    // port: u16,
}

#[derive(PartialEq, Serialize, Deserialize, Default, Clone, Debug)]
struct LocalState {
    num_held_inputs: u8,
    num_held_outputs: u8,
    held_input: Option<HeldInputJack>,
    held_output: Option<HeldOutputJack>,
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
pub trait Network<const I: usize, const O: usize> {
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
    fn jack_connect(&mut self, jack_id: usize, addr: [u8; 4], time: i64) -> Result<(), Error>;
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
pub struct Module<T: Network<I, O>, R: RngCore, const I: usize, const O: usize> {
    uuid: Uuid,
    color: u16,
    interface: T,
    leader_election: LeaderElection<R>,
    input_patch_enabled: u16,
    output_patch_enabled: u16,
    input_buffer: [AudioPacket; I],
    output_buffer: [AudioPacket; O],
    dropped_packets: u32,
    patch_state: PatchState,
    input_colors: [u16; I],
}

impl<T: Network<I, O>, R: RngCore, const I: usize, const O: usize> Module<T, R, I, O> {
    pub fn new(interface: T, rand_source: R, id: Uuid, color: u16, time: i64) -> Self {
        let leader_election = LeaderElection::new(id.clone(), time, rand_source);
        Module {
            uuid: id,
            color,
            interface,
            leader_election,
            input_patch_enabled: 0,
            output_patch_enabled: 0,
            input_buffer: [Default::default(); I],
            output_buffer: [Default::default(); O],
            dropped_packets: 0,
            patch_state: PatchState::Idle,
            input_colors: [0; I],
        }
    }

    pub fn poll<F>(&mut self, time: i64, f: F) -> Result<([Srgb<u8>; I], [Srgb<u8>; O]), Error>
    where
        F: FnOnce(&[AudioPacket; I], &mut [AudioPacket; O]),
    {
        let mut input_colors: [Srgb<u8>; I] = [Default::default(); I];
        let mut output_colors: [Srgb<u8>; O] = [Default::default(); O];
        self.interface.poll(time)?;
        if self.can_send() {
            if let Ok(d) = self.recv_directive() {
                match d {
                    Directive::GlobalStateUpdate(gsu) => {
                        self.process_gsu(gsu, time);
                    }
                    d => {
                        if let Some(resp) = self.leader_election.poll(Some(d), time) {
                            self.send_directive(&resp)?;
                            if let Directive::GlobalStateUpdate(gsu) = resp {
                                // Some network interfaces don't send multicast messages back to the
                                // client, so we process again in case we are the leader.
                                self.process_gsu(gsu, time);
                            }
                        }
                    }
                }
            }
            if let Some(resp) = self.leader_election.poll(None, time) {
                self.send_directive(&resp)?;
                if let Directive::GlobalStateUpdate(gsu) = resp {
                    // Some network interfaces don't send multicast messages back to the
                    // client, so we process again in case we are the leader.
                    self.process_gsu(gsu, time);
                }
            }
            for i in 0..I {
                if let Ok(a) = self.jack_recv(i) {
                    self.input_buffer[i] = a;
                    let avg = self.input_buffer[i].avg();
                    let c: Srgb =
                        Hsv::new(self.input_colors[i] as f32, 1.0, avg * 16.0 / i16::MAX as f32).into_color();
                    input_colors[i] = c.into_format();
                } else {
                    self.dropped_packets += 1;
                }
            }
            f(&self.input_buffer, &mut self.output_buffer);
            for i in 0..O {
                let buf = self.output_buffer[i];
                self.jack_send(i, &buf).unwrap();
                let avg = self.output_buffer[i].avg();
                let c: Srgb =
                    Hsv::new(self.color as f32, 1.0, avg * 16.0 / i16::MAX as f32).into_color();
                output_colors[i] = c.into_format();
            }
        } else {
            self.leader_election.reset(time);
        }
        self.interface.poll(time)?;
        if time % 10000 == 0 && self.dropped_packets != 0 {
            info!("{} dropped packets: {:?}", self.uuid, self.dropped_packets);
            self.dropped_packets = 0;
        }

        let color: Srgb<u8> = match self.patch_state {
            PatchState::Idle => Default::default(),
            PatchState::PatchEnabled => Srgb::new(255, 255, 255),
            PatchState::PatchToggled => Srgb::new(255, 255, 0),
            PatchState::Blocked => Srgb::new(255, 0, 0),
        };
        match self.patch_state {
            PatchState::Idle => Ok((input_colors, output_colors)),
            _ => Ok(([color; I], [color; O])),
        }
    }

    pub fn can_send(&mut self) -> bool {
        self.interface.can_send()
    }

    fn recv_directive(&mut self) -> Result<Directive, Error> {
        let mut buf = [0; 2048];
        match self.interface.recv_directive(&mut buf) {
            Ok(size) => match postcard::from_bytes(&buf[0..size]) {
                Ok(out) => {
                    trace!("<= {:?}", out);
                    Ok(out)
                }
                Err(e) => {
                    info!("Postcard Parse Error: {:?}", e);
                    Err(Error::Parse)
                }
            },
            Err(_) => Err(Error::NoData),
        }
    }

    fn send_directive(&mut self, directive: &Directive) -> Result<(), Error> {
        trace!("=> {:?}", directive);
        let mut buf = [0; 2048];
        match postcard::to_slice(directive, &mut buf) {
            Ok(res) => self.interface.send_directive(res),
            Err(e) => {
                info!("Postcard Parse Error: {:?}", e);
                Err(Error::Parse)
            }
        }
    }

    pub fn jack_connect(&mut self, jack_id: usize, addr: [u8; 4], time: i64) -> Result<(), Error> {
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
        if jack_id >= I {
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
        if jack_id >= O {
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
        for i in 0..I {
            if (self.input_patch_enabled & (1 << i)) != 0 {
                if local_state.held_input.is_none() {
                    local_state.held_input = Some(HeldInputJack {
                        uuid: self.uuid.clone(),
                        id: i as u32,
                    });
                }
                local_state.num_held_inputs += 1;
            }
        }
        for i in 0..O {
            if (self.output_patch_enabled & (1 << i)) != 0 {
                if local_state.held_output.is_none() {
                    local_state.held_output = Some(HeldOutputJack {
                        uuid: self.uuid.clone(),
                        id: i as u32,
                        color: self.color,
                        addr: self.interface.jack_addr(i)?,
                    });
                }
                local_state.num_held_outputs += 1;
            }
        }
        self.leader_election.update_local_state(local_state);
        Ok(())
    }

    fn process_gsu(&mut self, gsu: DirectiveGlobalStateUpdate, time: i64) {
        self.patch_state = gsu.patch_state;
                        if let Some(input) = gsu.input {
                            if input.uuid == self.uuid
                                && gsu.patch_state == PatchState::PatchToggled
                            {
                                if let Some(output) = gsu.output {
                                    self.toggle_input_jack(input.id as usize, output, time);
                                }
                            }
                        }
    }

    fn toggle_input_jack(&mut self, jack_id: usize, output: HeldOutputJack, time: i64) {
        // For now this is just a switch rather than a toggle
        match self.interface.jack_connect(jack_id, output.addr, time) {
            Ok(_) => self.input_colors[jack_id] = output.color,
            Err(e) => info!("Jack connection error: {:?}", e),
        }
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
