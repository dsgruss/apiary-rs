use crate::{
    Directive,
    Directive::{
        GlobalStateUpdate, Heartbeat, HeartbeatResponse, RequestVote, RequestVoteResponse,
    },
    DirectiveGlobalStateUpdate, DirectiveHeartbeat, DirectiveHeartbeatResponse,
    DirectiveRequestVote, DirectiveRequestVoteResponse, Error, HeldInputJack, HeldOutputJack,
    LocalState, PatchState, Uuid,
};
use heapless::FnvIndexMap;
use rand_core::RngCore;

const ELECTION_TIMEOUT_INTERVAL: (i64, i64) = (150, 300); // ms
const HEARTBEAT_INTERVAL: i64 = 50; // ms
const MAX_HOSTS: usize = 16;

#[derive(PartialEq, Debug)]
enum Roles {
    Follower,
    Candidate,
    Leader,
}

pub(crate) struct LeaderElection<T: RngCore> {
    id: Uuid,
    seen_hosts: FnvIndexMap<Uuid, Option<LocalState>, MAX_HOSTS>,
    rand_source: T,
    local_state: LocalState,
    election_timeout: i64,
    heartbeat_timeout: i64,
    current_term: u32,
    voted_for: Option<Uuid>,
    role: Roles,
    votes_got: u32,
    iteration: u32,
    last_update: Option<Directive>,
    last_seen_hosts: Option<usize>,
}

impl<T: RngCore> LeaderElection<T> {
    pub(crate) fn new(id: Uuid, time: i64, mut rand_source: T) -> Self {
        let seen_hosts = FnvIndexMap::<_, _, MAX_HOSTS>::new();

        let election_timeout = (rand_source.next_u32() as i64)
            % (ELECTION_TIMEOUT_INTERVAL.1 - ELECTION_TIMEOUT_INTERVAL.0)
            + ELECTION_TIMEOUT_INTERVAL.0
            + time;

        LeaderElection {
            id,
            seen_hosts,
            rand_source,
            local_state: Default::default(),
            election_timeout,
            heartbeat_timeout: HEARTBEAT_INTERVAL + time,
            current_term: 0,
            voted_for: None,
            role: Roles::Follower,
            votes_got: 0,
            iteration: 0,
            last_update: None,
            last_seen_hosts: Some(0),
        }
    }

    pub(crate) fn reset(&mut self, time: i64) {
        self.reset_election_timer(time);
        self.reset_heartbeat_timer(time);
        self.role = Roles::Follower;
    }

    fn reset_election_timer(&mut self, time: i64) {
        self.election_timeout = (self.rand_source.next_u32() as i64)
            % (ELECTION_TIMEOUT_INTERVAL.1 - ELECTION_TIMEOUT_INTERVAL.0)
            + ELECTION_TIMEOUT_INTERVAL.0
            + time;
    }

    fn reset_heartbeat_timer(&mut self, time: i64) {
        self.heartbeat_timeout = HEARTBEAT_INTERVAL + time;
    }

    fn election_timer_elapsed(&self, time: i64) -> bool {
        time > self.election_timeout
    }

    fn heartbeat_timer_elapsed(&self, time: i64) -> bool {
        time > self.heartbeat_timeout
    }

    pub(crate) fn poll(&mut self, message: Option<Directive>, time: i64) -> Option<Directive> {
        if self.check_message(&message).is_err() {
            return None;
        }

        self.seen_hosts
            .insert(self.id.clone(), Some(self.local_state.clone()))
            .unwrap();

        match message {
            Some(Heartbeat(hb)) => {
                if hb.term < self.current_term {
                    Some(self.heartbeat_response_fail(self.current_term))
                } else {
                    if hb.term > self.current_term || self.role == Roles::Candidate {
                        self.current_term = hb.term;
                        self.role = Roles::Follower;
                        self.voted_for = Some(hb.uuid.clone());
                    }
                    self.reset_election_timer(time);
                    /*
                    info!(
                        "{}: Heartbeat from {}, election timer now at {}",
                        time, uuid, self.election_timeout
                    );
                    */
                    Some(self.heartbeat_response_success(self.current_term, hb.iteration))
                }
            }
            Some(RequestVote(rv)) => {
                if rv.term < self.current_term {
                    Some(self.vote_response(self.current_term, rv.uuid, false))
                } else {
                    if rv.term > self.current_term {
                        self.current_term = rv.term;
                        self.role = Roles::Follower;
                        self.voted_for = Some(rv.uuid.clone());
                    }
                    Some(match &self.voted_for {
                        None => self.vote_response(rv.term, rv.uuid, true),
                        Some(i) if *i == rv.uuid => self.vote_response(rv.term, rv.uuid, true),
                        _ => self.vote_response(rv.term, rv.uuid, false),
                    })
                }
            }
            resp => match self.role {
                Roles::Follower => {
                    if self.election_timer_elapsed(time) {
                        self.role = Roles::Candidate;
                        self.current_term += 1;
                        self.voted_for = Some(self.id.clone());
                        self.seen_hosts.clear();
                        self.seen_hosts
                            .insert(self.id.clone(), Some(self.local_state.clone()))
                            .unwrap();
                        self.votes_got = 1;
                        self.reset_election_timer(time);
                        self.reset_heartbeat_timer(time);
                        Some(RequestVote(DirectiveRequestVote {
                            uuid: self.id.clone(),
                            term: self.current_term,
                        }))
                    } else {
                        None
                    }
                }
                Roles::Candidate => {
                    if let Some(RequestVoteResponse(rvr)) = resp {
                        if rvr.term == self.current_term && rvr.voted_for == self.id {
                            if rvr.vote_granted {
                                self.votes_got += 1;
                            } else {
                                self.role = Roles::Follower;
                            }
                        }
                    }
                    if self.heartbeat_timer_elapsed(time) {
                        if 2 * self.votes_got / self.seen_hosts.len() as u32 >= 1 {
                            info!("{:?} has been elected leader", self.id);
                            self.role = Roles::Leader;
                            self.iteration = 0;
                        } else {
                            self.role = Roles::Follower;
                        }
                    }
                    None
                }
                Roles::Leader => {
                    if let Some(HeartbeatResponse(DirectiveHeartbeatResponse {
                        uuid: id,
                        term: _,
                        success: true,
                        iteration: Some(i),
                        state: Some(s),
                    })) = resp
                    {
                        if i == self.iteration {
                            // A timeout value should be added here for modules that go offline
                            self.seen_hosts.insert(id, Some(s)).unwrap();

                            // If everyone known checked in, then send update
                            if Some(self.seen_hosts.len()) == self.last_seen_hosts {
                                let result = self.check_global_state_update();
                                self.last_seen_hosts = None;
                                result
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else if self.heartbeat_timer_elapsed(time) {
                        // Currently, this sends an update every heartbeat, meaning it could be up
                        // to 100 ms before a change in the patch status is registered. Future
                        // mitigations would be to use a third timer for the heartbeat response
                        // and/or send the state update as soon as all known module have responded.
                        if self.last_seen_hosts.is_some() {
                            if let Some(result) = self.check_global_state_update() {
                                return Some(result);
                            }
                        }

                        self.reset_heartbeat_timer(time);
                        self.last_seen_hosts = Some(self.seen_hosts.len());
                        self.seen_hosts.clear();
                        self.seen_hosts
                            .insert(self.id.clone(), Some(self.local_state.clone()))
                            .unwrap();
                        self.iteration += 1;
                        Some(Heartbeat(DirectiveHeartbeat {
                            uuid: self.id.clone(),
                            term: self.current_term,
                            iteration: self.iteration,
                        }))
                    } else {
                        None
                    }
                }
            },
        }
    }

    fn check_message(&mut self, message: &Option<Directive>) -> Result<(), Error> {
        let result = match message {
            Some(Heartbeat(m)) => {
                self.seen_hosts.insert(m.uuid.clone(), None).is_err() || m.uuid == self.id
            }
            Some(HeartbeatResponse(m)) => {
                self.seen_hosts.insert(m.uuid.clone(), None).is_err() || m.uuid == self.id
            }
            Some(RequestVote(m)) => {
                self.seen_hosts.insert(m.uuid.clone(), None).is_err() || m.uuid == self.id
            }
            Some(RequestVoteResponse(m)) => {
                self.seen_hosts.insert(m.uuid.clone(), None).is_err() || m.uuid == self.id
            }
            Some(_) => true,
            None => false,
        };
        if result {
            Err(Error::General)
        } else {
            Ok(())
        }
    }

    fn check_global_state_update(&mut self) -> Option<Directive> {
        let mut input_jack = None;
        let mut output_jack = None;
        let mut input_jack_count = 0;
        let mut output_jack_count = 0;
        for local_state in self.seen_hosts.values().flatten() {
            if local_state.num_held_inputs == 1 && input_jack.is_none() {
                input_jack = local_state.held_input.clone();
            }
            if local_state.num_held_outputs == 1 && output_jack.is_none() {
                output_jack = local_state.held_output.clone();
            }
            input_jack_count += local_state.num_held_inputs;
            output_jack_count += local_state.num_held_outputs;
        }

        let update = Some(match (input_jack_count, output_jack_count) {
            (0, 0) => self.gsu(PatchState::Idle, None, None),
            (1, 0) => self.gsu(PatchState::PatchEnabled, input_jack, None),
            (0, 1) => self.gsu(PatchState::PatchEnabled, None, output_jack),
            (1, 1) => self.gsu(PatchState::PatchToggled, input_jack, output_jack),
            _ => self.gsu(PatchState::Blocked, None, None),
        });
        if update != self.last_update {
            info!("Sending global update: {:?}", update);
            self.last_update = update.clone();
            update
        } else {
            None
        }
    }

    fn heartbeat_response_fail(&self, term: u32) -> Directive {
        HeartbeatResponse(DirectiveHeartbeatResponse {
            uuid: self.id.clone(),
            term,
            success: false,
            iteration: None,
            state: None,
        })
    }

    fn heartbeat_response_success(&self, term: u32, iteration: u32) -> Directive {
        HeartbeatResponse(DirectiveHeartbeatResponse {
            uuid: self.id.clone(),
            term,
            success: true,
            iteration: Some(iteration),
            state: Some(self.local_state.clone()),
        })
    }

    fn vote_response(&self, term: u32, voted_for: Uuid, vote_granted: bool) -> Directive {
        RequestVoteResponse(DirectiveRequestVoteResponse {
            uuid: self.id.clone(),
            term,
            voted_for,
            vote_granted,
        })
    }

    fn gsu(
        &self,
        patch_state: PatchState,
        input: Option<HeldInputJack>,
        output: Option<HeldOutputJack>,
    ) -> Directive {
        GlobalStateUpdate(DirectiveGlobalStateUpdate {
            uuid: self.id.clone(),
            patch_state,
            input,
            output,
        })
    }

    pub(crate) fn update_local_state(&mut self, local_state: LocalState) {
        self.local_state = local_state;
    }
}
