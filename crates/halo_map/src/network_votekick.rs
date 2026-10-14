//! The players' vote to kick a player (OpenCE's
//! port/linux/game/network_votekick.c, port/linux/NETCODE.md).
//!
//! A player votes against another from the scoreboard or the console; the
//! host alone counts, trusting nothing a client says but which player it
//! votes against. One vote a person (machines of one address, or one
//! hardware id, are one person); time played on this host to start a vote
//! and to vote; more than half of those who may vote, and at least two; one
//! vote at a time, then a gap; a failed vote's starter cools down and its
//! target is protected; a machine kicked by a vote (or leaving during one)
//! is kept out for a while. A client hears the host's count for the
//! scoreboard.
//!
//! The steps: the host taking a request ([`request`]: a client's message or
//! its own machine's), its tick ([`host_tick`]), a client hearing the count
//! ([`status`]) and the host's server's questions ([`machine`]: a machine
//! joined, one kept out, a new game). The records are the game's memory
//! (network_votekick_records.h); what the steps read of the game is gathered
//! before them; a machine's players' names, notices, the kick, the count
//! sent and the log are the game's, through [`AiHost::call`]
//! (network_votekick.c's `network_votekick_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};
use crate::layout::{
    distributed_votekick_request as rq, distributed_votekick_status as st, net_values as nv,
    votekick_identity as id, votekick_inputs as vi, votekick_state as vs, votekick_texts as tx, votekick_vote as vv,
};

/// network_votekick.c's calls (halo_sim.h's `_network_votekick_call_*`).
pub mod call {
    /// The machine `a`'s players' names into the texts' names, or (`b`)
    /// the vote's target's.
    pub const NAMES: i32 = 1;
    /// The texts' notice to everyone.
    pub const NOTICE: i32 = 2;
    /// ... to the machine `a` alone (NONE: the host's own).
    pub const REFUSE: i32 = 3;
    /// The machine `a` kicked.
    pub const KICK: i32 = 4;
    /// The texts' message (the count) to the machine `a`.
    pub const SEND_STATUS: i32 = 5;
    /// The log's line: a vote started by the machine `a` against `b`.
    pub const LOG_STARTED: i32 = 6;
}

/// halo_sim.h's struct halo_sim_network_votekick kinds.
const REQUEST_OWN: i32 = 1;
const MACHINE_JOINED: i32 = 0;
const MACHINE_KEPT_OUT: i32 = 1;
const MACHINE_NEW_GAME: i32 = 2;

const VOTE_SECONDS: i32 = 45;
const VOTE_GAP_SECONDS: u32 = 30;
const STARTER_COOLDOWN_SECONDS: u32 = 5 * 60;
const TARGET_PROTECTION_SECONDS: u32 = 10 * 60;
const VOTER_MINIMUM_SECONDS: i32 = 2 * 60;
const REQUEST_INTERVAL_MILLISECONDS: u32 = 1000;
const MINIMUM_VOTES: i16 = 2;
const TICKS_PER_SECOND: i32 = 30;
const MACHINES: i32 = nv::MAXIMUM_VOTEKICK_MACHINES as i32;
const REMEMBERED: u32 = nv::MAXIMUM_VOTEKICK_REMEMBERED as u32;
const HARDWARE_ID_SIZE: usize = nv::VOTEKICK_HARDWARE_ID_SIZE;
const NOTICE_SIZE: usize = nv::VOTEKICK_NOTICE_SIZE;
const NO_PLAYER: u8 = 0xff;
/// The status's flags.
const STATUS_VOTED: u8 = 1 << 0;
const STATUS_MAY_VOTE: u8 = 1 << 1;

/// halo_sim.h's `struct halo_sim_network_votekick`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    pub kind: i32,
    pub machine_index: i32,
    pub player_index: i32,
    pub from_stream: i32,
    pub entries: u32,
    pub state: u32,
    pub texts: u32,
    pub inputs: u32,
    pub host: i32,
    pub result: i32,
}

/// The step's arguments, read from the game's memory.
fn arguments(context: &WorldContext, m: &dyn AiMemory) -> Arguments {
    let mut bytes = [0u8; std::mem::size_of::<Arguments>()];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = m.r8(context.arguments + index as u32);
    }
    // SAFETY: plain data of its size
    unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const Arguments) }
}

/// network_votekick.c's `votekick_machine_valid`.
fn machine_valid(machine_index: i32) -> bool {
    (0..MACHINES).contains(&machine_index)
}

/// `PIN`.
fn pin(value: i32, low: i32, high: i32) -> i32 {
    value.max(low).min(high)
}

/// network_votekick.c's `votekick_time_reached`: whether a time
/// (milliseconds) has come; times wrap, so by their difference.
fn time_reached(now: u32, time: u32) -> bool {
    now.wrapping_sub(time) as i32 >= 0
}

/// A string at the address, to its end (at most `size` bytes).
fn c_string(m: &dyn AiMemory, address: u32, size: usize) -> Vec<u8> {
    (0..size as u32).map(|offset| m.r8(address + offset)).take_while(|&byte| byte != 0).collect()
}

/// `csstrcpy`: the string and its end at the address.
fn c_string_set(m: &dyn AiMemory, address: u32, text: &[u8]) {
    for (offset, byte) in text.iter().enumerate() {
        m.w8(address + offset as u32, *byte);
    }
    m.w8(address + text.len() as u32, 0);
}

/// p2p.c's `p2p_hardware_id_sanitize`: the id's hex digits, in lower case,
/// at most the size's (and the id's 32).
fn hardware_id_sanitize(source: &[u8], size: usize) -> Vec<u8> {
    source
        .iter()
        .map(|&character| if character.is_ascii_uppercase() && character <= b'F' { character - b'A' + b'a' } else { character })
        .filter(|character| character.is_ascii_digit() || (b'a'..=b'f').contains(character))
        .take((size - 1).min(32))
        .collect()
}

/// network_votekick.c's `struct votekick_voter`: a machine as the count
/// sees it.
#[derive(Debug, Clone, Default)]
struct Voter {
    address: u32,
    hardware_id: Vec<u8>,
    host: bool,
    target: bool,
    eligible: bool,
    voted: bool,
}

/// `votekick_same_address`: the host's own is only itself; an address not
/// known, no other's.
fn same_address(a: &Voter, b: &Voter) -> bool {
    if a.host || b.host {
        return a.host && b.host;
    }
    a.address != 0 && a.address == b.address
}

/// `votekick_same_person`: or by hardware id too (none told: no other's).
fn same_person(a: &Voter, b: &Voter) -> bool {
    same_address(a, b) || (!a.host && !b.host && !a.hardware_id.is_empty() && a.hardware_id == b.hardware_id)
}

/// `votekick_may_vote`: played long enough, and its address known.
fn may_vote(voter: &Voter) -> bool {
    voter.eligible && !voter.target && (voter.host || voter.address != 0)
}

/// `votekick_count`: the votes for (by address and hardware id, none of the
/// target's person) and the players who may vote (by address: the
/// target's among them, once).
fn count(voters: &[Voter]) -> (i16, i16) {
    let mut electorate = 0;
    for (index, voter) in voters.iter().enumerate() {
        let counted = (may_vote(voter) || voter.target)
            && !voters[..index].iter().any(|other| (may_vote(other) || other.target) && same_address(voter, other));
        if counted {
            electorate += 1;
        }
    }
    let mut votes = 0;
    for (index, voter) in voters.iter().enumerate() {
        let mut counted = voter.voted && may_vote(voter);
        for (other_index, other) in voters.iter().enumerate() {
            if !counted {
                break;
            }
            if other.target && same_person(voter, other) {
                counted = false;
            }
            if other_index < index && other.voted && may_vote(other) && same_person(voter, other) {
                counted = false;
            }
        }
        if counted {
            votes += 1;
        }
    }
    (votes, electorate)
}

/// `votekick_votes_needed`: more than half of those who may vote, and at
/// least `MINIMUM_VOTES`.
fn votes_needed(electorate: i16) -> i16 {
    (electorate / 2 + 1).max(MINIMUM_VOTES)
}

struct Vote<'a, H: AiHost> {
    host: &'a H,
    m: &'a dyn AiMemory,
    a: Arguments,
}

impl<H: AiHost> Vote<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32) {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        self.host.call(&mut call);
    }

    fn input(&self, field: usize) -> u32 {
        self.a.inputs + field as u32
    }

    fn state(&self, field: usize) -> u32 {
        self.a.state + field as u32
    }

    fn vote(&self, field: usize) -> u32 {
        self.a.state + vs::VOTE as u32 + field as u32
    }

    fn now(&self) -> u32 {
        self.m.r32(self.input(vi::NOW))
    }

    fn active(&self) -> bool {
        self.m.r8(self.vote(vv::ACTIVE)) != 0
    }

    fn target_names(&self) -> String {
        String::from_utf8_lossy(&c_string(self.m, self.vote(vv::TARGET_NAMES), nv::VOTEKICK_NAMES_SIZE)).into_owned()
    }

    /// `snprintf` into the texts' notice: at most its size, with its end.
    fn notice_set(&self, text: &str) {
        let bytes = text.as_bytes();
        c_string_set(self.m, self.a.texts + tx::NOTICE as u32, &bytes[..bytes.len().min(NOTICE_SIZE - 1)]);
    }

    /// `votekick_notice`: the notice, to everyone.
    fn notice(&self, text: &str) {
        self.notice_set(text);
        self.call(call::NOTICE, 0, 0);
    }

    /// `votekick_refuse`: a refusal, to the asking machine alone.
    fn refuse(&self, machine_index: i32, text: &str) {
        self.notice_set(text);
        self.call(call::REFUSE, machine_index, 0);
    }

    /// `votekick_starter_ticks`, `votekick_voter_ticks`: the time played to
    /// start a vote, and to vote (in ticks).
    fn starter_ticks(&self) -> i32 {
        pin(self.m.i32(self.input(vi::MINUTES)), 0, 60) * 60 * TICKS_PER_SECOND
    }

    fn voter_ticks(&self) -> i32 {
        self.starter_ticks().min(VOTER_MINIMUM_SECONDS * TICKS_PER_SECOND)
    }

    fn played_ticks(&self, machine_index: i32) -> i32 {
        self.m.i32(self.state(vs::PLAYED_TICKS) + 4 * machine_index as u32)
    }

    fn generation(&self, machine_index: i32) -> i32 {
        self.m.i32(self.state(vs::GENERATIONS) + 4 * machine_index as u32)
    }

    /// `votekick_voter_get`: a machine (NONE: the host's own) as the count
    /// sees it.
    fn voter(&self, machine_index: i32) -> Voter {
        let m = self.m;
        if machine_index == NONE {
            return Voter { host: true, eligible: true, voted: self.active() && m.r8(self.vote(vv::HOST_VOTED)) != 0, ..Voter::default() };
        }
        let slot = machine_index as u32;
        Voter {
            address: m.r32(self.input(vi::ADDRESSES) + 4 * slot),
            hardware_id: c_string(m, self.input(vi::HARDWARE_IDS) + slot * HARDWARE_ID_SIZE as u32, HARDWARE_ID_SIZE),
            host: false,
            eligible: self.played_ticks(machine_index) >= self.voter_ticks(),
            target: self.active() && machine_index == m.i32(self.vote(vv::TARGET_MACHINE)),
            voted: self.active()
                && m.r8(self.vote(vv::VOTED) + slot) != 0
                && m.i32(self.vote(vv::VOTED_GENERATIONS) + 4 * slot) == self.generation(machine_index),
        }
    }

    fn machines(&self) -> Vec<i32> {
        let count = self.m.i16(self.input(vi::MACHINE_COUNT)).max(0) as u32;
        (0..count).map(|index| self.m.i32(self.input(vi::MACHINE_INDICES) + 4 * index)).collect()
    }

    fn clients(&self) -> Vec<i32> {
        let count = self.m.i16(self.input(vi::CLIENT_COUNT)).max(0) as u32;
        (0..count).map(|index| self.m.i32(self.input(vi::CLIENT_INDICES) + 4 * index)).collect()
    }

    /// `votekick_target_present`: the target's machine still the one voted
    /// against, in the game.
    fn target_present(&self) -> bool {
        let target = self.m.i32(self.vote(vv::TARGET_MACHINE));
        self.generation(target) == self.m.i32(self.vote(vv::TARGET_GENERATION)) && self.machines().contains(&target)
    }

    /// `votekick_recount`: the count now, of the machines in the game.
    fn recount(&self) {
        let voters: Vec<Voter> = self.machines().iter().map(|&machine_index| self.voter(machine_index)).collect();
        let (votes, electorate) = count(&voters);
        self.m.set_i16(self.vote(vv::VOTES), votes);
        self.m.set_i16(self.vote(vv::NEEDED), votes_needed(electorate));
    }

    fn votes(&self) -> i16 {
        self.m.i16(self.vote(vv::VOTES))
    }

    fn needed(&self) -> i16 {
        self.m.i16(self.vote(vv::NEEDED))
    }

    // ---------- remembered identities

    /// `votekick_remember`: in a free entry, one gone by, else the one that
    /// ends soonest.
    fn remember(&self, identities: usize, address: u32, hardware_id: &[u8], milliseconds: u32) {
        let m = self.m;
        let now = self.now();
        let entry = |index: u32| self.state(identities) + index * id::SIZEOF as u32;
        let mut oldest = 0;
        for index in 0..REMEMBERED {
            let identity = entry(index);
            if m.r8(identity + id::VALID as u32) == 0 || time_reached(now, m.r32(identity + id::UNTIL as u32)) {
                oldest = index;
                break;
            }
            if (m.r32(identity + id::UNTIL as u32).wrapping_sub(m.r32(entry(oldest) + id::UNTIL as u32)) as i32) < 0 {
                oldest = index;
            }
        }
        let identity = entry(oldest);
        m.w8(identity + id::VALID as u32, 1);
        m.w32(identity + id::ADDRESS as u32, address);
        c_string_set(m, identity + id::HARDWARE_ID as u32, &hardware_id_sanitize(hardware_id, HARDWARE_ID_SIZE));
        m.w32(identity + id::UNTIL as u32, now.wrapping_add(milliseconds));
    }

    /// `votekick_remembered`: whether an address or hardware id is
    /// remembered (not gone by), and the milliseconds left.
    fn remembered(&self, identities: usize, address: u32, hardware_id: &[u8]) -> Option<u32> {
        let m = self.m;
        let now = self.now();
        (0..REMEMBERED).map(|index| self.state(identities) + index * id::SIZEOF as u32).find_map(|identity| {
            let until = m.r32(identity + id::UNTIL as u32);
            if m.r8(identity + id::VALID as u32) == 0 || time_reached(now, until) {
                return None;
            }
            let remembered_id = c_string(m, identity + id::HARDWARE_ID as u32, HARDWARE_ID_SIZE);
            ((address != 0 && m.r32(identity + id::ADDRESS as u32) == address)
                || (!hardware_id.is_empty() && !remembered_id.is_empty() && hardware_id == remembered_id.as_slice()))
            .then(|| until.wrapping_sub(now))
        })
    }

    // ---------- the vote's end

    /// `votekick_send_status`: the count to every client, what it may do
    /// and has.
    fn send_status(&self) {
        let m = self.m;
        let now = self.now();
        for machine_index in self.clients() {
            let message = self.a.texts + tx::MESSAGE as u32;
            let status = self.a.texts + tx::MESSAGE_STATUS as u32;
            for offset in 0..nv::VOTEKICK_MESSAGE_SIZE as u32 {
                m.w8(message + offset, 0);
            }
            m.w8(status + st::PLAYER_INDEX as u32, NO_PLAYER);
            if self.active() {
                let elapsed = now.wrapping_sub(m.r32(self.vote(vv::STARTED_AT)));
                let left = VOTE_SECONDS - (elapsed / 1000) as i32;
                let voter = self.voter(machine_index);
                let target_player = m.i16(self.vote(vv::TARGET_PLAYER));
                // (distributed_player_to_byte)
                m.w8(status + st::PLAYER_INDEX as u32, if target_player != NONE as i16 { target_player as u8 } else { NO_PLAYER });
                m.w8(status + st::VOTES as u32, pin(self.votes() as i32, 0, 255) as u8);
                m.w8(status + st::NEEDED as u32, pin(self.needed() as i32, 0, 255) as u8);
                m.w8(status + st::SECONDS_LEFT as u32, pin(left, 0, 255) as u8);
                let mut flags = 0;
                if voter.voted {
                    flags |= STATUS_VOTED;
                }
                if may_vote(&voter) {
                    flags |= STATUS_MAY_VOTE;
                }
                m.w8(status + st::FLAGS as u32, flags);
            }
            self.call(call::SEND_STATUS, machine_index, 0);
        }
    }

    /// `votekick_end`: the vote over, and none for a while.
    fn end(&self) {
        self.m.w8(self.vote(vv::ACTIVE), 0);
        self.m.w32(self.state(vs::NEXT_ALLOWED), self.now().wrapping_add(VOTE_GAP_SECONDS * 1000));
        self.m.w8(self.state(vs::GAP), 1);
        self.send_status();
    }

    fn target_identity(&self) -> (u32, Vec<u8>) {
        (self.m.r32(self.vote(vv::TARGET_ADDRESS)), c_string(self.m, self.vote(vv::TARGET_HARDWARE_ID), HARDWARE_ID_SIZE))
    }

    /// `votekick_pass`: the player kicked and kept out; or (left) the player
    /// left during it, which passes it too, and is kept out as well.
    fn pass(&self, left: bool) {
        let ban_minutes = pin(self.m.i32(self.input(vi::BAN_MINUTES)), 1, 24 * 60);
        let plural = if ban_minutes == 1 { "" } else { "s" };
        if left {
            self.notice(&format!("{} left during the vote to kick them, kept out for {} minute{}", self.target_names(), ban_minutes, plural));
        } else {
            self.notice(&format!(
                "{} kicked by vote ({} of {}), kept out for {} minute{}",
                self.target_names(),
                self.votes(),
                self.needed(),
                ban_minutes,
                plural
            ));
        }
        let (address, hardware_id) = self.target_identity();
        self.remember(vs::KEPT_OUT, address, &hardware_id, (ban_minutes as u32).wrapping_mul(60 * 1000));
        if !left {
            self.call(call::KICK, self.m.i32(self.vote(vv::TARGET_MACHINE)), 0);
        }
        self.end();
    }

    /// `votekick_fail`.
    fn fail(&self) {
        self.notice(&format!("The vote to kick {} failed ({} of {})", self.target_names(), self.votes(), self.needed()));
        let starter_id = c_string(self.m, self.vote(vv::STARTER_HARDWARE_ID), HARDWARE_ID_SIZE);
        self.remember(vs::COOLING_STARTERS, self.m.r32(self.vote(vv::STARTER_ADDRESS)), &starter_id, STARTER_COOLDOWN_SECONDS * 1000);
        let (address, hardware_id) = self.target_identity();
        self.remember(vs::PROTECTED_TARGETS, address, &hardware_id, TARGET_PROTECTION_SECONDS * 1000);
        self.end();
    }

    /// The names the call put in the texts.
    fn names(&self, machine_index: i32) -> String {
        self.call(call::NAMES, machine_index, 0);
        String::from_utf8_lossy(&c_string(self.m, self.a.texts + tx::NAMES as u32, nv::VOTEKICK_NAMES_SIZE)).into_owned()
    }

    fn mark_voted(&self, machine_index: i32) {
        if machine_index == NONE {
            self.m.w8(self.vote(vv::HOST_VOTED), 1);
        } else {
            let slot = machine_index as u32;
            self.m.w8(self.vote(vv::VOTED) + slot, 1);
            self.m.w32(self.vote(vv::VOTED_GENERATIONS) + 4 * slot, self.generation(machine_index) as u32);
        }
    }

    /// `votekick_host_request`: a machine's vote against a player (NONE: the
    /// host's own machine): a new vote, or one for the vote running.
    fn host_request(&self, machine_index: i32, player_index: i16) {
        let m = self.m;
        let player_valid = m.r8(self.input(vi::PLAYER_VALID)) != 0;
        let target_machine = if player_valid { m.i32(self.input(vi::PLAYER_MACHINE)) } else { NONE };
        let now = self.now();
        if m.r8(self.input(vi::ENABLED)) == 0 {
            return self.refuse(machine_index, "votekick: the host has turned votes off");
        }
        if machine_index == NONE && m.r8(self.input(vi::HOST_HAS_PLAYER)) == 0 {
            return;
        }
        if !player_valid || m.r8(self.input(vi::PLAYER_QUIT)) != 0 {
            return self.refuse(machine_index, "votekick: no such player");
        }
        if target_machine == NONE {
            return self.refuse(machine_index, "votekick: the host's players cannot be voted out");
        }
        if !machine_valid(target_machine) || target_machine == machine_index {
            return self.refuse(machine_index, "votekick: not against your own players");
        }
        let voter = self.voter(machine_index);
        let mut target = self.voter(target_machine);
        target.target = true;
        if same_person(&voter, &target) {
            return self.refuse(machine_index, "votekick: not against your own machine");
        }
        if !voter.host && voter.address == 0 {
            return self.refuse(machine_index, "votekick: the host does not know your address");
        }
        // a vote for the vote running
        if self.active() {
            if target_machine != m.i32(self.vote(vv::TARGET_MACHINE)) {
                return self.refuse(machine_index, &format!("votekick: wait for the vote to kick {} to end", self.target_names()));
            }
            if !may_vote(&voter) {
                return self.refuse(machine_index, "votekick: you have not played long enough on this server to vote");
            }
            if voter.voted {
                return;
            }
            self.mark_voted(machine_index);
            let votes = self.votes();
            self.recount();
            // (told only as the count changes)
            if self.votes() != votes && self.votes() < self.needed() {
                let names = self.names(machine_index);
                self.notice(&format!("{} voted to kick {} ({} of {})", names, self.target_names(), self.votes(), self.needed()));
            }
            if self.votes() >= self.needed() {
                self.pass(false);
            } else {
                self.send_status();
            }
            return;
        }
        // a new vote
        let next_allowed = m.r32(self.state(vs::NEXT_ALLOWED));
        if m.r8(self.state(vs::GAP)) != 0 && !time_reached(now, next_allowed) {
            let seconds = next_allowed.wrapping_sub(now).wrapping_add(999) / 1000;
            return self.refuse(machine_index, &format!("votekick: the next vote can start in {} seconds", seconds));
        }
        if !voter.host && self.played_ticks(machine_index) < self.starter_ticks() {
            let minutes = self.starter_ticks() / (60 * TICKS_PER_SECOND);
            return self.refuse(
                machine_index,
                &format!(
                    "votekick: play {} minute{} on this server to start a vote ({} played)",
                    minutes,
                    if minutes == 1 { "" } else { "s" },
                    self.played_ticks(machine_index) / (60 * TICKS_PER_SECOND)
                ),
            );
        }
        if !voter.host {
            if let Some(left) = self.remembered(vs::COOLING_STARTERS, voter.address, &voter.hardware_id) {
                return self.refuse(machine_index, &format!("votekick: your last vote failed: wait {} seconds", left.wrapping_add(999) / 1000));
            }
        }
        if let Some(left) = self.remembered(vs::PROTECTED_TARGETS, target.address, &target.hardware_id) {
            return self.refuse(
                machine_index,
                &format!("votekick: a vote against that player failed lately: wait {} seconds", left.wrapping_add(999) / 1000),
            );
        }
        for offset in 0..vv::SIZEOF as u32 {
            m.w8(self.vote(0) + offset, 0);
        }
        m.w8(self.vote(vv::ACTIVE), 1);
        m.w32(self.vote(vv::TARGET_MACHINE), target_machine as u32);
        m.w32(self.vote(vv::TARGET_GENERATION), self.generation(target_machine) as u32);
        m.set_i16(self.vote(vv::TARGET_PLAYER), player_index);
        m.w32(self.vote(vv::TARGET_ADDRESS), target.address);
        c_string_set(m, self.vote(vv::TARGET_HARDWARE_ID), &target.hardware_id);
        self.call(call::NAMES, target_machine, 1);
        m.w32(self.vote(vv::STARTER_ADDRESS), voter.address);
        c_string_set(m, self.vote(vv::STARTER_HARDWARE_ID), &voter.hardware_id);
        m.w32(self.vote(vv::STARTED_AT), now);
        self.mark_voted(machine_index);
        self.recount();
        let names = self.names(machine_index);
        self.notice(&format!(
            "{} started a vote to kick {} ({} of {}): open the scores and pick the name to vote",
            names,
            self.target_names(),
            self.votes(),
            self.needed()
        ));
        self.call(call::LOG_STARTED, machine_index, target_machine);
        if self.votes() >= self.needed() {
            self.pass(false);
        } else {
            self.send_status();
        }
    }
}

/// The world step of the host taking a vote: network_votekick.c's
/// `network_votekick_handle_request` (a client's, from its own stream, at
/// most one a second) or the host's own machine's
/// (`network_votekick_request`), then `votekick_host_request`.
pub fn request(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    let vote = Vote { host, m, a };
    if a.kind == REQUEST_OWN {
        return vote.host_request(NONE, a.player_index as i16);
    }
    let machine_index = a.machine_index;
    if !machine_valid(machine_index) || a.from_stream == 0 {
        return;
    }
    let now = vote.now();
    let slot = machine_index as u32;
    let requested = vote.state(vs::REQUESTED) + slot;
    let request_time = vote.state(vs::REQUEST_TIMES) + 4 * slot;
    if m.r8(requested) != 0 && !time_reached(now, m.r32(request_time).wrapping_add(REQUEST_INTERVAL_MILLISECONDS)) {
        return;
    }
    m.w8(requested, 1);
    m.w32(request_time, now);
    let player_index = m.r8(a.entries + rq::PLAYER_INDEX as u32);
    if player_index as usize >= nv::MAXIMUM_TRACKED_PLAYERS {
        return;
    }
    vote.host_request(machine_index, player_index as i16);
}

/// The world step of the host's tick: network_votekick.c's
/// `network_votekick_host_tick`. The client machines' time played; the vote
/// passed if its target left; each second, recounted (passing), failed at
/// its end, or its count sent.
pub fn host_tick(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    let vote = Vote { host, m, a };
    for machine_index in vote.clients() {
        if machine_valid(machine_index) {
            let played = vote.state(vs::PLAYED_TICKS) + 4 * machine_index as u32;
            m.w32(played, m.i32(played).wrapping_add(1) as u32);
        }
    }
    if !vote.active() {
        return;
    }
    if !vote.target_present() {
        return vote.pass(true);
    }
    if m.i32(vote.input(vi::GAME_TIME)) % TICKS_PER_SECOND == 0 {
        vote.recount();
        if vote.votes() >= vote.needed() {
            return vote.pass(false);
        }
        if time_reached(vote.now(), m.r32(vote.vote(vv::STARTED_AT)).wrapping_add(VOTE_SECONDS as u32 * 1000)) {
            return vote.fail();
        }
        vote.send_status();
    }
}

/// The world step of a client hearing the host's count:
/// network_votekick.c's `network_votekick_handle_status`.
pub fn status(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    let vote = Vote { host, m, a };
    let status = vote.state(vs::STATUS);
    for offset in 0..st::SIZEOF as u32 {
        m.w8(status + offset, m.r8(a.entries + offset));
    }
    let mut heard = vote.now();
    if m.r8(status + st::PLAYER_INDEX as u32) != NO_PLAYER && m.r8(vote.input(vi::PLAYER_VALID)) == 0 {
        m.w8(status + st::PLAYER_INDEX as u32, NO_PLAYER);
    }
    // (0: none heard)
    if heard == 0 {
        heard = 1;
    }
    m.w32(vote.state(vs::STATUS_HEARD), heard);
}

/// The world step of the host's server's questions: network_votekick.c's
/// `network_votekick_machine_joined` (its time played starts over, and no
/// vote of the machine before is its), `network_votekick_kept_out` (a
/// machine joining kicked by a vote, or the one a vote runs against) and
/// `network_votekick_new_game` (a vote ends with its game).
pub fn machine(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    let vote = Vote { host, m, a };
    let machine_index = a.machine_index;
    match a.kind {
        MACHINE_JOINED => {
            if !machine_valid(machine_index) {
                return;
            }
            let slot = machine_index as u32;
            m.w32(vote.state(vs::PLAYED_TICKS) + 4 * slot, 0);
            let generation = vote.state(vs::GENERATIONS) + 4 * slot;
            m.w32(generation, m.i32(generation).wrapping_add(1) as u32);
            m.w8(vote.state(vs::REQUESTED) + slot, 0);
            m.w8(vote.vote(vv::VOTED) + slot, 0);
        }
        MACHINE_KEPT_OUT => {
            let address = m.r32(vote.input(vi::JOINING_ADDRESS));
            let hardware_id = c_string(m, vote.input(vi::JOINING_HARDWARE_ID), nv::VOTEKICK_JOINING_HARDWARE_ID_SIZE);
            let (target_address, target_id) = vote.target_identity();
            let result_address = context.arguments + std::mem::offset_of!(Arguments, result) as u32;
            if vote.active()
                && a.host != 0
                && ((address != 0 && address == target_address)
                    || (!hardware_id.is_empty() && !target_id.is_empty() && hardware_id == target_id))
            {
                m.w32(result_address, 1);
                return;
            }
            m.w32(result_address, vote.remembered(vs::KEPT_OUT, address, &hardware_id).is_some() as u32);
        }
        MACHINE_NEW_GAME => {
            if vote.active() && a.host != 0 {
                vote.notice("The vote ended with the game");
            }
            m.w8(vote.vote(vv::ACTIVE), 0);
            for offset in 0..st::SIZEOF as u32 {
                m.w8(vote.state(vs::STATUS) + offset, 0);
            }
            m.w32(vote.state(vs::STATUS_HEARD), 0);
        }
        _ => {}
    }
}
