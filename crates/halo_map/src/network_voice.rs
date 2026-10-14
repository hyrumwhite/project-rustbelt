//! Voice chat (OpenCE's port/linux/game/network_voice.c,
//! port/linux/NETCODE.md): each machine's microphone, encoded with Opus,
//! goes to the host, which sends it on to the machines that may hear it.
//!
//! The host decides everything, from its own settings: in the lobby every
//! player hears every other; in a game, by the voice mode (teammates near,
//! anyone near, all teammates, or all teammates and enemies near), a voice
//! heard for being near quieter further away and from the speaker's side;
//! the quality. It takes a machine's frames only with the key it gave it,
//! no larger than the quality allows, no more than so many a second, and
//! from no more than so many talkers at once; it sends voices only to a
//! machine that has said it hears them. Each machine decides only whether
//! it talks (push to talk, open mic), how loud the others are and whom it
//! mutes.
//!
//! The steps: each frame's update ([`update`]), the messages ([`message`]:
//! a client's frame to the host, a frame relayed to a client, the host's
//! settings) and a machine joining the host ([`joined`]). The records are
//! the game's memory (network_voice_records.h); what the steps read of the
//! game is gathered before them; sending, the microphone, Opus, playing and
//! a new key are the game's, through [`AiHost::call`] (network_voice.c's
//! `network_voice_call`).

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext};
use crate::layout::{
    distributed_message_header as mh, distributed_voice_config as cf, distributed_voice_down as dn,
    distributed_voice_up as up, net_values as nv, voice_inputs as vi, voice_machine as vm, voice_player as vp,
    voice_settings as vs, voice_state as st, voice_texts as tx,
};

/// network_voice.c's calls (halo_sim.h's `_network_voice_call_*`).
pub mod call {
    /// The texts' settings message to the machine `a`: whether sent.
    pub const SEND_CONFIG: i32 = 1;
    /// The texts' relayed frame (`b` bytes) to the machine `a`.
    pub const SEND_DOWN: i32 = 2;
    /// The texts' frame to the host (`b` bytes).
    pub const SEND_UP: i32 = 3;
    /// The packet at the address `b` (`input[2]` bytes) of the machine
    /// `a`'s frame `actor`, played at the gain `input[0]` and pan
    /// `input[1]`.
    pub const PLAY: i32 = 4;
    /// The microphone open if `a`.
    pub const MICROPHONE: i32 = 5;
    /// The microphone's next frame read: whether there was one.
    pub const READ_FRAME: i32 = 6;
    /// The frame's level: `output[0]`.
    pub const LEVEL: i32 = 7;
    /// The frame encoded at the bit rate `a` into the texts' packet, at
    /// most `b` bytes: its length.
    pub const ENCODE: i32 = 8;
    /// The voices heard forgotten.
    pub const FORGET_ALL: i32 = 9;
    /// The voices' volume `input[0]`.
    pub const SET_VOLUME: i32 = 10;
    /// Push to talk held.
    pub const PUSH_TO_TALK_HELD: i32 = 11;
    /// A new key for the machine slot `a`.
    pub const NEW_KEY: i32 = 12;
}

const VOICE_FRAMES_PER_SECOND: i32 = 50;
const VOICE_FRAME_BURST: i32 = 10;
const VOICE_MAXIMUM_TALKERS: i16 = 8;
const VOICE_TALKING_MS: u32 = 500;
const VOICE_LISTENING_MS: u32 = 6000;
const VOICE_HELLO_MS: u32 = 2000;
const VOICE_CONFIG_MS: u32 = 10000;
const VOICE_CONFIG_TIMEOUT_MS: u32 = 25000;
const VOICE_OPEN_MIC_HOLD_MS: u32 = 400;
const VOICE_MINIMUM_KBPS: i32 = 8;
const VOICE_MAXIMUM_KBPS: i32 = 64;
const VOICE_OPEN_MIC_LEVEL: f32 = 0.01;
const VOICE_NEAR_DISTANCE: f32 = 3.0;
const VOICE_FARTHEST_GAIN: f32 = 0.2;
const MACHINES: i32 = nv::VOICE_MACHINES as i32;
const MAXIMUM_PACKET: i32 = nv::VOICE_MAXIMUM_PACKET as i32;
const NO_PLAYER: u8 = 0xff;
/// real_math.h's `_real_epsilon`.
const REAL_EPSILON: f32 = 0.0001;
/// The settings' flag of voice in the lobby.
const CONFIG_LOBBY: u8 = 1 << 0;

/// network_voice.h's voice modes and routes.
const MODE_OFF: i16 = 0;
const MODE_TEAM_PROXIMITY: i16 = 1;
const MODE_TEAM_ENEMY_PROXIMITY: i16 = 2;
const MODE_TEAM_GLOBAL: i16 = 3;
const MODE_TEAM_GLOBAL_ENEMY_PROXIMITY: i16 = 4;
const NUMBER_OF_VOICE_MODES: i16 = 5;
const ROUTE_NONE: i16 = 0;
const ROUTE_GLOBAL: i16 = 1;
const ROUTE_PROXIMITY: i16 = 2;

/// halo_sim.h's `struct halo_sim_network_voice`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Arguments {
    pub machine_index: i32,
    pub message: u32,
    pub size: i32,
    pub state: u32,
    pub texts: u32,
    pub inputs: u32,
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

/// network_voice.c's `voice_route`: how a listener hears a speaker.
fn route(mode: i16, lobby: bool, lobby_voice: bool, teammates: bool, both_alive: bool, distance: f32, range: f32) -> i16 {
    let near = both_alive && distance <= range;
    if lobby {
        return if lobby_voice { ROUTE_GLOBAL } else { ROUTE_NONE };
    }
    match mode {
        MODE_TEAM_PROXIMITY if teammates && near => ROUTE_PROXIMITY,
        MODE_TEAM_ENEMY_PROXIMITY if near => ROUTE_PROXIMITY,
        MODE_TEAM_GLOBAL if teammates => ROUTE_GLOBAL,
        MODE_TEAM_GLOBAL_ENEMY_PROXIMITY if teammates => ROUTE_GLOBAL,
        MODE_TEAM_GLOBAL_ENEMY_PROXIMITY if near => ROUTE_PROXIMITY,
        _ => ROUTE_NONE,
    }
}

/// network_voice.c's `voice_packet_limit`: the largest packet a frame may
/// be at the quality.
fn packet_limit(kbps: i16) -> i32 {
    (kbps as i32 * 1000 / 8 / VOICE_FRAMES_PER_SECOND * 2 + 16).min(MAXIMUM_PACKET)
}

/// network_voice.c's `voice_mode_from_text`.
fn mode_from_text(text: &[u8]) -> i16 {
    const NAMES: [&[u8]; 5] = [b"off", b"team_proximity", b"team_enemy_proximity", b"team_global", b"team_global_enemy_proximity"];
    NAMES.iter().position(|name| *name == text).map_or(MODE_TEAM_GLOBAL_ENEMY_PROXIMITY, |mode| mode as i16)
}

fn machine_valid(machine_index: i32) -> bool {
    (0..MACHINES).contains(&machine_index)
}

/// A string at the address, to its end (at most `size` bytes).
fn c_string(m: &dyn AiMemory, address: u32, size: usize) -> Vec<u8> {
    (0..size as u32).map(|offset| m.r8(address + offset)).take_while(|&byte| byte != 0).collect()
}

type Vector3 = [f32; 3];

fn dot(a: Vector3, b: Vector3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// real_math.h's `square_root`.
fn square_root(x: f32) -> f32 {
    (x as f64).sqrt() as f32
}

struct Voice<'a, H: AiHost> {
    host: &'a H,
    m: &'a dyn AiMemory,
    a: Arguments,
}

impl<H: AiHost> Voice<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32, actor: i32, input: &[f32]) -> AiCall {
        let mut call = AiCall { kind, actor, a, b, ..AiCall::default() };
        call.input[..input.len()].copy_from_slice(input);
        self.host.call(&mut call);
        call
    }

    fn input(&self, field: usize) -> u32 {
        self.a.inputs + field as u32
    }

    fn flag(&self, field: usize) -> bool {
        self.m.r8(self.input(field)) != 0
    }

    fn state(&self, field: usize) -> u32 {
        self.a.state + field as u32
    }

    fn machine(&self, machine_index: i32, field: usize) -> u32 {
        self.state(st::MACHINES) + machine_index as u32 * vm::SIZEOF as u32 + field as u32
    }

    fn now(&self) -> u32 {
        self.m.r32(self.input(vi::NOW))
    }

    fn local_machine(&self) -> i32 {
        self.m.i32(self.input(vi::LOCAL_MACHINE))
    }

    fn settings(&self, field: usize) -> u32 {
        self.state(st::SETTINGS) + field as u32
    }

    fn host_settings(&self, field: usize) -> u32 {
        self.state(st::HOST_SETTINGS) + field as u32
    }

    fn player(&self, index: i16) -> u32 {
        self.input(vi::PLAYERS) + index as u32 * vp::SIZEOF as u32
    }

    // ---------- settings

    /// `voice_host_settings_read`: the host's, from its settings (read again
    /// as they change).
    fn host_settings_read(&self) {
        let m = self.m;
        let changes = m.r32(self.input(vi::CONFIG_CHANGES));
        if m.r32(self.state(st::HOST_SETTINGS_READ_AT)) == changes {
            return;
        }
        m.w32(self.state(st::HOST_SETTINGS_READ_AT), changes);
        m.w8(self.host_settings(vs::LOBBY), m.r8(self.input(vi::LOBBY_SETTING)));
        let text = c_string(m, self.input(vi::MODE_TEXT), nv::VOICE_SETTING_TEXT_SIZE);
        m.set_i16(self.host_settings(vs::MODE), mode_from_text(&text));
        let kbps = m.i32(self.input(vi::KBPS_SETTING)).clamp(VOICE_MINIMUM_KBPS, VOICE_MAXIMUM_KBPS);
        m.set_i16(self.host_settings(vs::KBPS), kbps as i16);
        let bits = (m.r32(self.input(vi::PROXIMITY_SETTING)) as u64) | ((m.r32(self.input(vi::PROXIMITY_SETTING) + 4) as u64) << 32);
        let proximity = f64::from_bits(bits);
        // (PIN, a NaN passing through)
        let proximity = if proximity < 5.0 { 5.0 } else if proximity > 100.0 { 100.0 } else { proximity };
        m.set_f32(self.host_settings(vs::PROXIMITY), proximity as f32);
    }

    // ---------- players

    /// `voice_machine_player`: a machine's first player in the game
    /// (absolute index), or NONE.
    fn machine_player(&self, machine_index: i32) -> i16 {
        let m = self.m;
        if !self.flag(vi::GAME_IN_PROGRESS) {
            return NONE as i16;
        }
        (0..m.i16(self.input(vi::PLAYER_COUNT)).max(0))
            .find(|&index| {
                let player = self.player(index);
                m.r8(player + vp::VALID as u32) != 0
                    && m.r8(player + vp::QUIT as u32) == 0
                    && m.i32(player + vp::MACHINE_INDEX as u32) == machine_index
            })
            .unwrap_or(NONE as i16)
    }

    /// `voice_route_between`: how a listener's machine hears a speaker's,
    /// by their first players.
    fn route_between(&self, speaker: i16, listener: i16) -> i16 {
        let m = self.m;
        let mode = m.i16(self.host_settings(vs::MODE));
        let lobby_voice = m.r8(self.host_settings(vs::LOBBY)) != 0;
        if self.flag(vi::IN_LOBBY) {
            return route(mode, true, lobby_voice, false, false, 0.0, 0.0);
        }
        if speaker == NONE as i16 || listener == NONE as i16 {
            return ROUTE_NONE;
        }
        let (speaker, listener) = (self.player(speaker), self.player(listener));
        let teammates = !self.flag(vi::GAME_ENGINE)
            || (self.flag(vi::GAME_ENGINE_HAS_TEAMS)
                && m.i32(speaker + vp::TEAM_INDEX as u32) == m.i32(listener + vp::TEAM_INDEX as u32));
        let both_alive = m.r8(speaker + vp::ALIVE as u32) != 0 && m.r8(listener + vp::ALIVE as u32) != 0;
        let mut distance = 0.0;
        if both_alive {
            let a = m.vector(speaker + vp::POSITION as u32);
            let b = m.vector(listener + vp::POSITION as u32);
            let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            distance = square_root(dot(v, v));
        }
        route(mode, false, lobby_voice, teammates, both_alive, distance, m.f32(self.host_settings(vs::PROXIMITY)))
    }

    // ---------- the host

    /// network_voice.c's `voice_machine_joined_game`: a new key.
    fn machine_joined(&self, machine_index: i32) {
        if !machine_valid(machine_index) {
            return;
        }
        for offset in 0..vm::SIZEOF as u32 {
            self.m.w8(self.machine(machine_index, 0) + offset, 0);
        }
        self.call(call::NEW_KEY, machine_index, 0, NONE, &[]);
    }

    /// `voice_host_tell`: the settings and its key, to one client.
    fn host_tell(&self, machine_index: i32) {
        let m = self.m;
        let message = self.a.texts + tx::CONFIG_MESSAGE as u32;
        for offset in 0..nv::VOICE_CONFIG_MESSAGE_SIZE as u32 {
            m.w8(message + offset, 0);
        }
        let config = self.a.texts + tx::CONFIG_MESSAGE_CONFIG as u32;
        m.w32(config + cf::KEY as u32, m.r32(self.machine(machine_index, vm::KEY)));
        m.w8(config + cf::FLAGS as u32, if m.r8(self.host_settings(vs::LOBBY)) != 0 { CONFIG_LOBBY } else { 0 });
        m.w8(config + cf::MODE as u32, m.i16(self.host_settings(vs::MODE)) as u8);
        m.w8(config + cf::KBPS as u32, m.i16(self.host_settings(vs::KBPS)) as u8);
        m.w16(config + cf::PROXIMITY_TENTHS as u32, (m.f32(self.host_settings(vs::PROXIMITY)) * 10.0) as i32 as u16);
        if self.call(call::SEND_CONFIG, machine_index, 0, NONE, &[]).result != 0 {
            m.w8(self.machine(machine_index, vm::TOLD), 1);
            m.w32(self.machine(machine_index, vm::TOLD_AT), self.now());
        }
    }

    /// `voice_proximity_gain`: the gain and pan of a voice heard for being
    /// near (this machine's own view, and the speaker's unit).
    fn proximity_gain(&self, player_index: i16) -> (f32, f32) {
        let m = self.m;
        let proximity = m.f32(self.settings(vs::PROXIMITY));
        let range = if proximity > VOICE_NEAR_DISTANCE { proximity } else { VOICE_NEAR_DISTANCE + 1.0 };
        let valid = player_index >= 0
            && player_index < m.i16(self.input(vi::PLAYER_COUNT))
            && m.r8(self.player(player_index) + vp::VALID as u32) != 0;
        if !valid || m.r8(self.player(player_index) + vp::ALIVE as u32) == 0 || !self.flag(vi::LOCAL_PLAYER) || !self.flag(vi::CAMERA) {
            return (1.0, 0.0);
        }
        let position = m.vector(self.player(player_index) + vp::POSITION as u32);
        let camera = m.vector(self.input(vi::CAMERA_POSITION));
        let mut direction = [position[0] - camera[0], position[1] - camera[1], position[2] - camera[2]];
        let distance = square_root(dot(direction, direction));
        let (mut gain, mut pan) = (1.0, 0.0);
        if distance > VOICE_NEAR_DISTANCE {
            let t = (distance - VOICE_NEAR_DISTANCE) / (range - VOICE_NEAR_DISTANCE);
            let t = if t < 0.0 { 0.0 } else if t > 1.0 { 1.0 } else { t };
            gain = 1.0 - (1.0 - VOICE_FARTHEST_GAIN) * t;
        }
        if distance > 0.01 {
            let a = m.vector(self.input(vi::CAMERA_FORWARD));
            let b = m.vector(self.input(vi::CAMERA_UP));
            let mut right = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
            // (normalize3d)
            let magnitude = square_root(dot(right, right));
            if !(REAL_EPSILON > magnitude.abs()) {
                let scale = 1.0 / magnitude;
                right = right.map(|part| scale * part);
            }
            let scale = 1.0 / distance;
            direction = direction.map(|part| scale * part);
            pan = 0.8 * dot(direction, right);
        }
        (gain, pan)
    }

    /// `voice_play`: a frame heard here.
    fn play(&self, machine_index: i32, player_index: i16, route: i16, sequence: u16, packet: u32, length: i32) {
        let m = self.m;
        if !machine_valid(machine_index) || length <= 0 {
            return;
        }
        let slot = machine_index as u32;
        m.w32(self.state(st::HEARD_AT) + 4 * slot, self.now());
        m.w8(self.state(st::HEARD) + slot, 1);
        if m.r8(self.state(st::MUTED) + slot) != 0 {
            return;
        }
        let (gain, pan) = if route == ROUTE_PROXIMITY { self.proximity_gain(player_index) } else { (1.0, 0.0) };
        self.call(call::PLAY, machine_index, packet as i32, sequence as i32, &[gain, pan, length as f32]);
    }

    /// `voice_host_talkers`: how many machines talk now, but the one.
    fn host_talkers(&self, except: i32, now: u32) -> i16 {
        (0..MACHINES)
            .filter(|&index| {
                index != except
                    && self.m.r8(self.machine(index, vm::TALKED)) != 0
                    && now.wrapping_sub(self.m.r32(self.machine(index, vm::TALKED_AT))) < VOICE_TALKING_MS
            })
            .count() as i16
    }

    /// `voice_host_relay`: a frame of a machine (its own, or a client's),
    /// checked, and sent to those that hear it.
    fn host_relay(&self, speaker: i32, sequence: u16, packet: u32, length: i32) {
        let m = self.m;
        let now = self.now();
        if !machine_valid(speaker) || length <= 0 || length > packet_limit(m.i16(self.host_settings(vs::KBPS))) {
            return;
        }
        let talking = m.r8(self.machine(speaker, vm::TALKED)) != 0
            && now.wrapping_sub(m.r32(self.machine(speaker, vm::TALKED_AT))) < VOICE_TALKING_MS;
        if !talking && self.host_talkers(speaker, now) >= VOICE_MAXIMUM_TALKERS {
            return;
        }
        m.w8(self.machine(speaker, vm::TALKED), 1);
        m.w32(self.machine(speaker, vm::TALKED_AT), now);
        let in_lobby = self.flag(vi::IN_LOBBY);
        let speaker_player = if in_lobby { NONE as i16 } else { self.machine_player(speaker) };
        let local_machine = self.local_machine() as i16 as i32;
        for index in 0..m.i16(self.input(vi::MACHINE_COUNT)).max(0) as u32 {
            let listener = m.i32(self.input(vi::MACHINE_INDICES) + 4 * index);
            if !machine_valid(listener) || listener == speaker {
                continue;
            }
            let route = self.route_between(speaker_player, if in_lobby { NONE as i16 } else { self.machine_player(listener) });
            if route == ROUTE_NONE {
                continue;
            }
            if listener == local_machine {
                self.play(speaker, speaker_player, route, sequence, packet, length);
                continue;
            }
            if m.r8(self.machine(listener, vm::HEARD)) == 0
                || now.wrapping_sub(m.r32(self.machine(listener, vm::HEARD_AT))) >= VOICE_LISTENING_MS
                || m.r8(self.machine(listener, vm::TOLD)) == 0
            {
                continue;
            }
            let message = self.a.texts + tx::DOWN_MESSAGE as u32;
            for offset in 0..(mh::SIZEOF + dn::SIZEOF) as u32 {
                m.w8(message + offset, 0);
            }
            let down = self.a.texts + tx::DOWN_MESSAGE_DOWN as u32;
            m.w32(down + dn::KEY as u32, m.r32(self.machine(listener, vm::KEY)));
            m.w16(down + dn::SEQUENCE as u32, sequence);
            m.w8(down + dn::MACHINE_INDEX as u32, speaker as u8);
            m.w8(down + dn::PLAYER_INDEX as u32, if speaker_player != NONE as i16 { speaker_player as u8 } else { NO_PLAYER });
            m.w8(down + dn::ROUTE as u32, route as u8);
            m.w8(down + dn::LENGTH as u32, length as u8);
            let target = self.a.texts + tx::DOWN_MESSAGE_PACKET as u32;
            for offset in 0..length as u32 {
                m.w8(target + offset, m.r8(packet + offset));
            }
            self.call(call::SEND_DOWN, listener, (mh::SIZEOF + dn::SIZEOF) as i32 + length, NONE, &[]);
        }
    }

    /// `voice_host_handle_up`: a client's frame: its key, its allowance, the
    /// settings'.
    fn host_handle_up(&self, machine_index: i32, entry: u32, packet: u32, length: i32) {
        let m = self.m;
        let now = self.now();
        if !machine_valid(machine_index) {
            return;
        }
        let key = m.r32(self.machine(machine_index, vm::KEY));
        if key == 0 || m.r32(entry + up::KEY as u32) != key || m.r8(entry + up::LENGTH as u32) as i32 != length {
            return;
        }
        m.w8(self.machine(machine_index, vm::HEARD), 1);
        m.w32(self.machine(machine_index, vm::HEARD_AT), now);
        if length == 0 {
            return;
        }
        let allowance_at = self.machine(machine_index, vm::ALLOWANCE_AT);
        let allowance_address = self.machine(machine_index, vm::ALLOWANCE);
        let mut allowance = if m.r32(allowance_at) == 0 {
            VOICE_FRAME_BURST as f32
        } else {
            m.f32(allowance_address) + now.wrapping_sub(m.r32(allowance_at)) as f32 * VOICE_FRAMES_PER_SECOND as f32 / 1000.0
        };
        allowance = allowance.min(VOICE_FRAME_BURST as f32);
        m.w32(allowance_at, now);
        if allowance < 1.0 {
            m.set_f32(allowance_address, allowance);
            return;
        }
        m.set_f32(allowance_address, allowance - 1.0);
        self.host_relay(machine_index, m.r16(entry + up::SEQUENCE as u32), packet, length);
    }

    fn may_talk(&self) -> bool {
        if self.flag(vi::IN_LOBBY) {
            self.m.r8(self.settings(vs::LOBBY)) != 0
        } else {
            self.m.i16(self.settings(vs::MODE)) != MODE_OFF
        }
    }

    /// `voice_host_update`: every client told its key and the settings
    /// (again as they change, and now and then).
    fn host_update(&self) {
        let m = self.m;
        let now = self.now();
        self.host_settings_read();
        let host = self.state(st::HOST_SETTINGS);
        let told = self.state(st::HOST_SETTINGS_TOLD);
        let changed = (0..vs::SIZEOF as u32).any(|offset| m.r8(host + offset) != m.r8(told + offset));
        for offset in 0..vs::SIZEOF as u32 {
            m.w8(told + offset, m.r8(host + offset));
            m.w8(self.settings(0) + offset, m.r8(host + offset));
        }
        m.w8(self.state(st::SETTINGS_HEARD), 1);
        m.w32(self.state(st::SETTINGS_HEARD_AT), now);
        let local_machine = self.local_machine() as i16 as i32;
        for index in 0..m.i16(self.input(vi::MACHINE_COUNT)).max(0) as u32 {
            let machine_index = m.i32(self.input(vi::MACHINE_INDICES) + 4 * index);
            if !machine_valid(machine_index) || machine_index == local_machine {
                continue;
            }
            if m.r32(self.machine(machine_index, vm::KEY)) == 0 {
                self.machine_joined(machine_index);
            }
            if m.r8(self.machine(machine_index, vm::TOLD)) == 0
                || changed
                || now.wrapping_sub(m.r32(self.machine(machine_index, vm::TOLD_AT))) >= VOICE_CONFIG_MS
            {
                self.host_tell(machine_index);
            }
        }
    }

    /// `voice_send`: this machine's frame: the host's own relayed, a
    /// client's sent up (none: it hears voices).
    fn send(&self, packet: u32, length: i32) {
        let m = self.m;
        let sequence = m.r16(self.state(st::SEQUENCE));
        if self.flag(vi::HOST) {
            if length > 0 {
                self.host_relay(self.local_machine(), sequence, packet, length);
            }
        } else {
            let message = self.a.texts + tx::UP_MESSAGE as u32;
            for offset in 0..(mh::SIZEOF + up::SIZEOF) as u32 {
                m.w8(message + offset, 0);
            }
            let entry = self.a.texts + tx::UP_MESSAGE_UP as u32;
            m.w32(entry + up::KEY as u32, m.r32(self.state(st::KEY)));
            m.w16(entry + up::SEQUENCE as u32, sequence);
            m.w8(entry + up::LENGTH as u32, length as u8);
            let target = self.a.texts + tx::UP_MESSAGE_PACKET as u32;
            for offset in 0..length.max(0) as u32 {
                m.w8(target + offset, m.r8(packet + offset));
            }
            self.call(call::SEND_UP, 0, (mh::SIZEOF + up::SIZEOF) as i32 + length, NONE, &[]);
        }
        if length > 0 {
            m.w16(self.state(st::SEQUENCE), sequence.wrapping_add(1));
        }
    }

    /// `voice_end_session`: the microphone closed, voices forgotten.
    fn end_session(&self) {
        let m = self.m;
        if m.r8(self.state(st::SESSION)) == 0 {
            return;
        }
        m.w8(self.state(st::SESSION), 0);
        m.w8(self.state(st::MICROPHONE_WANTED), 0);
        self.call(call::MICROPHONE, 0, 0, NONE, &[]);
        self.call(call::FORGET_ALL, 0, 0, NONE, &[]);
        m.w8(self.state(st::SETTINGS_HEARD), 0);
        m.w8(self.state(st::TALKED), 0);
        for slot in 0..MACHINES as u32 {
            m.w8(self.state(st::HEARD) + slot, 0);
        }
    }
}

/// The world step of voice chat's update, each frame: network_voice.c's
/// `network_voice_update`. The voices' volume; outside a network game the
/// session ended; the host's settings told (by the host) or forgotten
/// unheard (by a client); mutes of a machine whose players changed
/// forgotten; a client's hello; the microphone wanted (open mic, or push to
/// talk from its first press) and its frames, while talking, encoded and
/// sent (relayed, by the host).
pub fn update(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    let voice = Voice { host, m, a };
    let chat = c_string(m, voice.input(vi::CHAT_TEXT), nv::VOICE_SETTING_TEXT_SIZE);
    let open_mic = chat == b"open_mic";
    let push_to_talk = chat == b"push_to_talk";
    let now = voice.now();
    let volume_bits = (m.r32(voice.input(vi::VOLUME_SETTING)) as u64) | ((m.r32(voice.input(vi::VOLUME_SETTING) + 4) as u64) << 32);
    let volume = f64::from_bits(volume_bits);
    let volume = if volume < 0.0 { 0.0 } else if volume > 2.0 { 2.0 } else { volume };
    voice.call(call::SET_VOLUME, 0, 0, NONE, &[volume as f32]);
    if !voice.flag(vi::NETWORK_GAME) || voice.local_machine() == NONE {
        return voice.end_session();
    }
    m.w8(voice.state(st::SESSION), 1);
    let heard = voice.state(st::SETTINGS_HEARD);
    if voice.flag(vi::HOST) {
        voice.host_update();
    } else if m.r8(heard) != 0 && now.wrapping_sub(m.r32(voice.state(st::SETTINGS_HEARD_AT))) >= VOICE_CONFIG_TIMEOUT_MS {
        m.w8(heard, 0);
    }
    // (mutes of a machine whose players changed are forgotten)
    let name_size = 2 * nv::VOICE_NAME_LENGTH as u32;
    for slot in 0..MACHINES as u32 {
        let muted = voice.state(st::MUTED) + slot;
        if m.r8(muted) == 0 {
            continue;
        }
        let name = voice.input(vi::MACHINE_NAMES) + slot * name_size;
        let kept = voice.state(st::MUTED_NAMES) + slot * name_size;
        if (0..name_size).any(|offset| m.r8(name + offset) != m.r8(kept + offset)) {
            m.w8(muted, 0);
        }
    }
    let wanted = voice.state(st::MICROPHONE_WANTED);
    if m.r8(heard) == 0 {
        voice.call(call::MICROPHONE, 0, 0, NONE, &[]);
        m.w8(wanted, 0);
        return;
    }
    // (a client says it hears voices)
    if !voice.flag(vi::HOST) && now.wrapping_sub(m.r32(voice.state(st::HELLO_AT))) >= VOICE_HELLO_MS {
        m.w32(voice.state(st::HELLO_AT), now);
        voice.send(0, 0);
    }
    let held = push_to_talk
        && voice.call(call::PUSH_TO_TALK_HELD, 0, 0, NONE, &[]).result != 0
        && !voice.flag(vi::CONSOLE_ACTIVE);
    if !voice.may_talk() || (!open_mic && !push_to_talk) {
        m.w8(wanted, 0);
    } else if open_mic || held {
        m.w8(wanted, 1);
    }
    voice.call(call::MICROPHONE, m.r8(wanted) as i32, 0, NONE, &[]);
    while m.r8(wanted) != 0 && voice.call(call::READ_FRAME, 0, 0, NONE, &[]).result != 0 {
        let mut talk = held;
        if open_mic {
            let level = voice.call(call::LEVEL, 0, 0, NONE, &[]).output[0];
            if level >= VOICE_OPEN_MIC_LEVEL {
                m.w32(voice.state(st::OPEN_MIC_UNTIL), now.wrapping_add(VOICE_OPEN_MIC_HOLD_MS));
            }
            talk = (m.r32(voice.state(st::OPEN_MIC_UNTIL)).wrapping_sub(now) as i32) > 0;
        }
        if !talk || !voice.may_talk() {
            continue;
        }
        let kbps = m.i16(voice.settings(vs::KBPS));
        let length = voice.call(call::ENCODE, kbps as i32 * 1000, packet_limit(kbps).min(MAXIMUM_PACKET), NONE, &[]).result;
        if length <= 0 {
            continue;
        }
        voice.send(a.texts + tx::PACKET as u32, length);
        m.w8(voice.state(st::TALKED), 1);
        m.w32(voice.state(st::TALKED_AT), now);
    }
}

/// The world step of a voice message: network_voice.c's
/// `network_voice_handle_message`. A client's frame to the host (checked
/// and relayed); a frame relayed to a client (with its key, played); the
/// host's settings and this machine's key, heard.
pub fn message(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    let voice = Voice { host, m, a };
    let kind = m.r8(a.message + mh::TYPE as u32) as usize;
    let entries = a.message + mh::SIZEOF as u32;
    let length = a.size - mh::SIZEOF as i32;
    let machine_index = a.machine_index;
    let is_host = voice.flag(vi::HOST);
    match kind {
        nv::_DISTRIBUTED_MESSAGE_VOICE_UP => {
            if machine_index == NONE || !is_host || length < up::SIZEOF as i32 {
                return;
            }
            voice.host_handle_up(machine_index, entries, entries + up::SIZEOF as u32, length - up::SIZEOF as i32);
        }
        nv::_DISTRIBUTED_MESSAGE_VOICE_DOWN => {
            if machine_index != NONE || is_host || m.r8(voice.state(st::SETTINGS_HEARD)) == 0 || length < dn::SIZEOF as i32 {
                return;
            }
            let down_length = m.r8(entries + dn::LENGTH as u32) as i32;
            let speaker = m.r8(entries + dn::MACHINE_INDEX as u32) as i32;
            if m.r32(entries + dn::KEY as u32) != m.r32(voice.state(st::KEY))
                || down_length != length - dn::SIZEOF as i32
                || speaker == voice.local_machine()
            {
                return;
            }
            let player = m.r8(entries + dn::PLAYER_INDEX as u32);
            let player = if player != NO_PLAYER && !voice.flag(vi::IN_LOBBY) { player as i16 } else { NONE as i16 };
            voice.play(
                speaker,
                player,
                m.r8(entries + dn::ROUTE as u32) as i16,
                m.r16(entries + dn::SEQUENCE as u32),
                entries + dn::SIZEOF as u32,
                down_length,
            );
        }
        nv::_DISTRIBUTED_MESSAGE_VOICE_CONFIG => {
            if machine_index != NONE || is_host || length < cf::SIZEOF as i32 {
                return;
            }
            m.w32(voice.state(st::KEY), m.r32(entries + cf::KEY as u32));
            m.w8(voice.settings(vs::LOBBY), (m.r8(entries + cf::FLAGS as u32) & CONFIG_LOBBY != 0) as u8);
            let mode = m.r8(entries + cf::MODE as u32) as i16;
            m.set_i16(voice.settings(vs::MODE), if mode < NUMBER_OF_VOICE_MODES { mode } else { MODE_OFF });
            let kbps = (m.r8(entries + cf::KBPS as u32) as i32).clamp(VOICE_MINIMUM_KBPS, VOICE_MAXIMUM_KBPS);
            m.set_i16(voice.settings(vs::KBPS), kbps as i16);
            m.set_f32(voice.settings(vs::PROXIMITY), m.r16(entries + cf::PROXIMITY_TENTHS as u32) as f32 / 10.0);
            m.w8(voice.state(st::SETTINGS_HEARD), 1);
            m.w32(voice.state(st::SETTINGS_HEARD_AT), voice.now());
        }
        _ => {}
    }
}

/// The world step of a machine joining the host: network_voice.c's
/// `network_voice_machine_joined`, a new key told to it before anything is
/// taken from it.
pub fn joined(context: &WorldContext, host: &impl AiHost) {
    let m = host.memory();
    let a = arguments(context, m);
    Voice { host, m, a }.machine_joined(a.machine_index);
}
