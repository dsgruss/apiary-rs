use crate::{
    Directive,
    Directive::{GlobalStateUpdate, HeartbeatResponse},
    DirectiveGlobalStateUpdate, DirectiveHeartbeatResponse, HeldInputJack, HeldOutputJack,
    LocalState, PatchState, Uuid,
};
use heapless::FnvIndexMap;

const HEARTBEAT_INTERVAL: i64 = 50; // ms
const MAX_HOSTS: usize = 16;

pub(crate) struct PingPatch {
    id: Uuid,
    seen_hosts: FnvIndexMap<Uuid, Option<LocalState>, MAX_HOSTS>,
    local_state: LocalState,
    heartbeat_timeout: i64,
    last_update: Option<Directive>,
}

impl PingPatch {
    pub(crate) fn new(id: Uuid, time: i64) -> Self {
        let seen_hosts = FnvIndexMap::<_, _, MAX_HOSTS>::new();

        PingPatch {
            id,
            seen_hosts,
            local_state: Default::default(),
            heartbeat_timeout: HEARTBEAT_INTERVAL + time,
            last_update: None,
        }
    }

    fn reset_heartbeat_timer(&mut self, time: i64) {
        self.heartbeat_timeout = HEARTBEAT_INTERVAL + time;
    }

    fn heartbeat_timer_elapsed(&self, time: i64) -> bool {
        time > self.heartbeat_timeout
    }

    pub(crate) fn poll(
        &mut self,
        message: Option<Directive>,
        time: i64,
    ) -> (Option<Directive>, Option<Directive>) {
        if let Some(HeartbeatResponse(resp)) = message {
            if resp.uuid != self.id {
                self.seen_hosts.insert(resp.uuid, resp.state).unwrap();
            }
        }
        let mut ping = None;
        let mut gsu = None;
        if self.heartbeat_timer_elapsed(time) {
            self.reset_heartbeat_timer(time);
            gsu = self.check_global_state_update();
            self.seen_hosts.clear();
            if self.local_state.num_held_inputs + self.local_state.num_held_outputs > 0 {
                self.seen_hosts
                    .insert(self.id.clone(), Some(self.local_state.clone()))
                    .unwrap();
                ping = Some(self.heartbeat_response_success(0, 0));
            }
        }
        (ping, gsu)
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

    fn heartbeat_response_success(&self, term: u32, iteration: u32) -> Directive {
        HeartbeatResponse(DirectiveHeartbeatResponse {
            uuid: self.id.clone(),
            term,
            success: true,
            iteration: Some(iteration),
            state: Some(self.local_state.clone()),
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
