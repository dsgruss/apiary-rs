use crate::protocol::{
    Directive,
    Directive::{Heartbeat, HeartbeatResponse, RequestVote, RequestVoteResponse},
    LocalState, Uuid,
};
use heapless::FnvIndexSet;
use rand_core::RngCore;

const ELECTION_TIMEOUT_INTERVAL: (i64, i64) = (150, 300); // ms
const HEARTBEAT_INTERVAL: i64 = 50; // ms
                                    // const RESPONSE_TIMEOUT: i64 = 50; // ms
const MAX_HOSTS: usize = 16;

#[derive(PartialEq, Debug)]
pub enum Roles {
    FOLLOWER,
    CANDIDATE,
    LEADER,
}

pub struct LeaderElection<'a> {
    id: Uuid,
    seen_hosts: FnvIndexSet<Uuid, MAX_HOSTS>,
    rand_source: &'a mut RngCore,
    local_state: LocalState,
    election_timeout: i64,
    heartbeat_timeout: i64,
    current_term: u32,
    voted_for: Option<Uuid>,
    pub role: Roles,
    votes_got: u32,
    iteration: u32,
}

impl<'a> LeaderElection<'a> {
    pub fn new(id: Uuid, time: i64, rand_source: &'a mut RngCore) -> Self {
        let seen_hosts = FnvIndexSet::<_, MAX_HOSTS>::new();

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
            role: Roles::FOLLOWER,
            votes_got: 0,
            iteration: 0,
        }
    }

    pub fn reset(&mut self, time: i64) {
        self.reset_election_timer(time);
        self.reset_heartbeat_timer(time);
        self.role = Roles::FOLLOWER;
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

    pub fn poll(&mut self, message: Option<Directive>, time: i64) -> Option<Directive> {
        match message {
            Some(Heartbeat {
                uuid,
                term,
                iteration,
            }) => {
                self.seen_hosts.insert(uuid.clone()).unwrap();
                if self.id == uuid {
                    None
                } else if term < self.current_term {
                    self.heartbeat_response_fail(self.current_term)
                } else {
                    if term > self.current_term || self.role == Roles::CANDIDATE {
                        self.current_term = term;
                        self.role = Roles::FOLLOWER;
                        self.voted_for = Some(uuid.clone());
                    }
                    self.reset_election_timer(time);
                    info!(
                        "{}: Heartbeat from {}, election timer now at {}",
                        time, uuid, self.election_timeout
                    );
                    self.heartbeat_response_success(self.current_term, iteration)
                }
            }
            Some(RequestVote { uuid, term }) => {
                self.seen_hosts.insert(uuid.clone()).unwrap();
                if self.id == uuid {
                    None
                } else if term < self.current_term {
                    self.vote_response(self.current_term, uuid, false)
                } else {
                    if term > self.current_term {
                        self.current_term = term;
                        self.role = Roles::FOLLOWER;
                        self.voted_for = None;
                    }
                    match &self.voted_for {
                        None => self.vote_response(term, uuid, true),
                        Some(i) if *i == uuid => self.vote_response(term, uuid, true),
                        _ => self.vote_response(term, uuid, false),
                    }
                }
            }
            resp => match self.role {
                Roles::FOLLOWER => {
                    if self.election_timer_elapsed(time) {
                        self.role = Roles::CANDIDATE;
                        self.current_term += 1;
                        self.voted_for = Some(self.id.clone());
                        self.seen_hosts.clear();
                        self.seen_hosts.insert(self.id.clone()).unwrap();
                        self.votes_got = 1;
                        self.reset_election_timer(time);
                        self.reset_heartbeat_timer(time);
                        Some(RequestVote {
                            uuid: self.id.clone(),
                            term: self.current_term,
                        })
                    } else {
                        None
                    }
                }
                Roles::CANDIDATE => {
                    if let Some(RequestVoteResponse {
                        uuid: _,
                        term,
                        voted_for,
                        vote_granted,
                    }) = resp
                    {
                        if term == self.current_term && voted_for == self.id {
                            if vote_granted {
                                self.votes_got += 1;
                            } else {
                                self.role = Roles::FOLLOWER;
                            }
                        }
                    }
                    if self.heartbeat_timer_elapsed(time) {
                        if 2 * self.votes_got / self.seen_hosts.len() as u32 >= 1 {
                            self.role = Roles::LEADER;
                            self.iteration = 0;
                        } else {
                            self.role = Roles::FOLLOWER;
                        }
                    }
                    None
                }
                Roles::LEADER => {
                    if self.heartbeat_timer_elapsed(time) {
                        self.reset_heartbeat_timer(time);
                        self.iteration += 1;
                        Some(Heartbeat {
                            uuid: self.id.clone(),
                            term: self.current_term,
                            iteration: self.iteration,
                        })
                    } else {
                        None
                    }
                }
            },
        }
    }

    fn heartbeat_response_fail(&self, term: u32) -> Option<Directive> {
        Some(HeartbeatResponse {
            uuid: self.id.clone(),
            term,
            success: false,
            iteration: None,
            state: None,
        })
    }

    fn heartbeat_response_success(&self, term: u32, iteration: u32) -> Option<Directive> {
        Some(HeartbeatResponse {
            uuid: self.id.clone(),
            term,
            success: true,
            iteration: Some(iteration),
            state: Some(self.local_state.clone()),
        })
    }

    fn vote_response(&self, term: u32, voted_for: Uuid, vote_granted: bool) -> Option<Directive> {
        Some(RequestVoteResponse {
            uuid: self.id.clone(),
            term,
            voted_for,
            vote_granted,
        })
    }
}
