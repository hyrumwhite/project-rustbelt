//! The conversations' update (ai_communication.c's `ai_conversation_update`,
//! every tick of the AI's update, with `ai_conversation_line_begin`,
//! `ai_conversation_line_perform` and `ai_conversation_line_end`): each
//! conversation tries to begin once a second until it can or gives up,
//! speaks its lines in turn (waiting for its speaker or everyone to come
//! near, for the speaker to be free, for the line to finish and its delay to
//! run out, and if the line asks, to be told to advance), finishes, and
//! points its participants' attention at the speaker or the addressee.
//!
//! The conversations and actors are the game's arrays, worked on in place
//! ([`WorldContext`]); beginning and finishing a conversation, the scripted
//! dialogue and sounds, and a unit's speech are the game's, through
//! [`AiHost::call`] (ai_communication.c's `conversation_call`, shared with
//! the C). The conversations' console lines (`ai_print_conversations`) are
//! the game's alone.

use crate::ai_actor::{AiCall, AiHost, AiMemory, AiMemoryExt, NONE, WorldContext, datum_get, object_get, tag_block_count, tag_block_element};
use crate::layout::{actor_datum as a, ai_conversation as ac, ai_values as v, conversation_datum as c, scenario, unit_datum as u};

/// The conversations' calls (halo_sim.h's `_conversation_call_*`).
pub mod call {
    /// `ai_conversation_begin(a, &continue_trying)`: whether to keep
    /// trying in `result2`.
    pub const BEGIN: i32 = 1;
    /// `ai_conversation_finish(a, b & 1, b & 2)`.
    pub const FINISH: i32 = 2;
    /// `sound_scripted_dialog_is_playing()`, the result.
    pub const SCRIPTED_DIALOG_PLAYING: i32 = 3;
    /// `scripted_sound_new(a, NONE, 1.0)`.
    pub const SCRIPTED_SOUND_NEW: i32 = 4;
    /// `unit_test_speech` of the unit `a` at a yell for the sound `b`: the
    /// play type, the result.
    pub const TEST_SPEECH: i32 = 5;
    /// `unit_speak` of the conversation `a`'s line (its speaker, addressee
    /// and sound) with the play type `b`.
    pub const SPEAK: i32 = 6;
    /// `scripted_sound_time(a)`, the result.
    pub const SCRIPTED_SOUND_TIME: i32 = 7;
}

/// ai_communication.c's maxima and local enums.
const MAXIMUM_PARTICIPANTS_PER_CONVERSATION: i32 = 8;
const LINE_ADDRESSEE_LOOK_BACK_BIT: u16 = 0;
const LINE_EVERYONE_LOOK_AT_SPEAKER_BIT: u16 = 1;
const LINE_EVERYONE_LOOK_AT_ADDRESSEE_BIT: u16 = 2;
const LINE_WAIT_AFTER_UNTIL_TOLD_TO_ADVANCE_BIT: u16 = 3;
const LINE_WAIT_UNTIL_SPEAKER_NEARBY_BIT: u16 = 4;
const LINE_WAIT_UNTIL_EVERYONE_NEARBY_BIT: u16 = 5;
const ADDRESS_PLAYER: i16 = 1;
const ADDRESS_PARTICIPANT: i16 = 2;
const SELECTION_RADIO: i16 = 6;
const SELECTION_RADIO_SARGEANT: i16 = 7;
const PRIORITY_YELL: i16 = 6;

/// ai_communication.c's `struct scenario_conversation_line_view` (its
/// offsets asserted in the C).
mod line {
    pub const SIZEOF: usize = 0x7C;
    pub const FLAGS: u32 = 0x0;
    pub const PARTICIPANT_INDEX: u32 = 0x2;
    pub const ADDRESS_TYPE: u32 = 0x4;
    pub const ADDRESS_PARTICIPANT_INDEX: u32 = 0x6;
    pub const DELAY_TIME: u32 = 0xC;
    pub const DIALOGUE: u32 = 0x1C;
    /// `struct tag_reference`: its size and its index's offset.
    pub const TAG_REFERENCE_SIZE: u32 = 0x10;
    pub const TAG_REFERENCE_INDEX: u32 = 0xC;
}

/// ai_communication.c's `struct scenario_conversation_participant_view`.
mod participant {
    pub const SIZEOF: usize = 0x54;
    pub const SELECTION_TYPE: u32 = 0x4;
}

/// `struct data_array`'s fields.
const DATA_SIZE: u32 = 0x22;
const DATA_COUNT: u32 = 0x2E;
const DATA_DATA: u32 = 0x34;

struct Conversations<'a, H: AiHost> {
    m: &'a dyn AiMemory,
    host: &'a H,
    context: &'a WorldContext,
}

impl<H: AiHost> Conversations<'_, H> {
    fn call(&self, kind: i32, a: i32, b: i32) -> AiCall {
        let mut call = AiCall { kind, actor: NONE, a, b, ..AiCall::default() };
        self.host.call(&mut call);
        call
    }

    fn conversation(&self, conversation_index: i32) -> u32 {
        datum_get(self.m, self.context.conversation_data, conversation_index).unwrap_or(0)
    }

    fn actor(&self, actor_index: i32) -> u32 {
        datum_get(self.m, self.context.actor_data, actor_index).unwrap_or(0)
    }

    /// The conversation's definition (the scenario's).
    fn definition(&self, conversation: u32) -> u32 {
        tag_block_element(
            self.m,
            self.context.scenario + scenario::AI_CONVERSATIONS as u32,
            self.m.i16(conversation + c::SCENARIO_CONVERSATION_INDEX as u32) as i32,
            ac::SIZEOF,
        )
    }

    /// `ai_conversation_participant_count`.
    fn participant_count(&self, definition: u32) -> i32 {
        tag_block_count(self.m, definition + ac::PARTICIPANTS as u32).min(MAXIMUM_PARTICIPANTS_PER_CONVERSATION)
    }

    fn actor_index_of(&self, conversation: u32, participant_index: i32) -> i32 {
        self.m.i32(conversation + c::ACTOR_INDICES as u32 + 4 * participant_index as u32)
    }

    /// `ai_conversation_line_begin`: the line's speaker, addressee, sound,
    /// delay and flags, if its participant takes part.
    fn line_begin(&self, conversation_index: i32) -> bool {
        let m = self.m;
        let conversation = self.conversation(conversation_index);
        let at = |offset: usize| conversation + offset as u32;
        let definition = self.definition(conversation);
        let line = tag_block_element(m, definition + ac::LINES as u32, m.i16(at(c::LINE_INDEX)) as i32, line::SIZEOF);
        let participant_index = m.i16(line + line::PARTICIPANT_INDEX);
        if !(participant_index >= 0
            && (participant_index as i32) < self.participant_count(definition)
            && m.r32(at(c::PARTICIPANT_BITMASK)) & (1 << participant_index) != 0)
        {
            return false;
        }
        let participant = tag_block_element(m, definition + ac::PARTICIPANTS as u32, participant_index as i32, participant::SIZEOF);
        let speaker_actor_index = self.actor_index_of(conversation, participant_index as i32);
        m.set_i16(at(c::LINE_PARTICIPANT_INDEX), participant_index);
        if speaker_actor_index == NONE {
            m.w32(at(c::LINE_ACTOR_INDEX), NONE as u32);
            m.w32(at(c::LINE_UNIT_INDEX), NONE as u32);
            m.w32(at(c::LINE_ADDRESS_UNIT_INDEX), NONE as u32);
            m.set_bool(at(c::LINE_UNSPATIALIZED), true);
        } else {
            let actor = self.actor(speaker_actor_index);
            m.w32(at(c::LINE_ACTOR_INDEX), speaker_actor_index as u32);
            m.w32(at(c::LINE_UNIT_INDEX), m.r32(actor + a::META_UNIT_INDEX as u32));
            m.w32(at(c::LINE_ADDRESS_UNIT_INDEX), NONE as u32);
            match m.i16(line + line::ADDRESS_TYPE) {
                ADDRESS_PLAYER => m.w32(at(c::LINE_ADDRESS_UNIT_INDEX), m.r32(at(c::TRIGGERING_PLAYER_UNIT_INDEX))),
                ADDRESS_PARTICIPANT => {
                    let address_index = m.i16(line + line::ADDRESS_PARTICIPANT_INDEX) as i32;
                    if address_index >= 0
                        && address_index < self.participant_count(definition)
                        && self.actor_index_of(conversation, address_index) != NONE
                    {
                        let address_actor = self.actor(self.actor_index_of(conversation, address_index));
                        m.w32(at(c::LINE_ADDRESS_UNIT_INDEX), m.r32(address_actor + a::META_UNIT_INDEX as u32));
                    }
                }
                _ => {}
            }
            let selection_type = m.i16(participant + participant::SELECTION_TYPE);
            m.set_bool(
                at(c::LINE_UNSPATIALIZED),
                selection_type == SELECTION_RADIO || selection_type == SELECTION_RADIO_SARGEANT,
            );
        }

        let dialogue_index = m.i16(at(c::DIALOGUE_INDICES) + 2 * participant_index as u32);
        let reference = line + line::DIALOGUE + line::TAG_REFERENCE_SIZE * dialogue_index as i32 as u32;
        m.w32(at(c::LINE_SOUND_INDEX), m.r32(reference + line::TAG_REFERENCE_INDEX));
        m.set_i16(at(c::LINE_DELAY_TIMER), (m.f32(line + line::DELAY_TIME) * 30.0) as i32 as i16);
        m.w16(at(c::LINE_FLAGS), m.r16(line + line::FLAGS));
        m.set_bool(at(c::LINE_ADVANCE), false);
        m.set_bool(at(c::LINE_FINISHED), false);
        m.set_bool(at(c::LINE_SPOKEN), false);
        true
    }

    /// `ai_conversation_line_perform`: whether the line is done and the
    /// conversation may go on.
    fn line_perform(&self, conversation_index: i32) -> bool {
        let m = self.m;
        let conversation = self.conversation(conversation_index);
        let at = |offset: usize| conversation + offset as u32;
        let definition = self.definition(conversation);
        if m.bool(at(c::LINE_ADVANCE)) {
            return true;
        }
        let flag = |bit: u16| m.r16(at(c::LINE_FLAGS)) & (1 << bit) != 0;

        if !m.bool(at(c::LINE_SPOKEN)) {
            let mut busy = false;
            let sound_index = m.i32(at(c::LINE_SOUND_INDEX));
            if sound_index != NONE {
                if flag(LINE_WAIT_UNTIL_SPEAKER_NEARBY_BIT) || flag(LINE_WAIT_UNTIL_EVERYONE_NEARBY_BIT) {
                    for participant_index in 0..self.participant_count(definition) {
                        let participant_actor_index = self.actor_index_of(conversation, participant_index);
                        if participant_actor_index == NONE {
                            continue;
                        }
                        let actor = self.actor(participant_actor_index);
                        if (flag(LINE_WAIT_UNTIL_EVERYONE_NEARBY_BIT)
                            || (flag(LINE_WAIT_UNTIL_SPEAKER_NEARBY_BIT)
                                && participant_actor_index == m.i32(at(c::LINE_ACTOR_INDEX))))
                            && m.i16(actor + a::STATE_ACTION as u32) == v::_ACTOR_ACTION_CONVERSE as i16
                            && m.i32(actor + a::STATE_CONVERSE_RUN_TO_UNIT_INDEX as u32) != NONE
                            && !m.bool(actor + a::STATE_CONVERSE_IN_RANGE as u32)
                            && !m.bool(actor + a::STATE_CONVERSE_FAILED as u32)
                        {
                            busy = true;
                        }
                    }
                }

                if self.call(call::SCRIPTED_DIALOG_PLAYING, 0, 0).result != 0 {
                    busy = true;
                }

                if !busy {
                    let speaking_unit_index = m.i32(at(c::LINE_UNIT_INDEX));
                    if speaking_unit_index == NONE || m.bool(at(c::LINE_UNSPATIALIZED)) {
                        self.call(call::SCRIPTED_SOUND_NEW, m.i32(at(c::LINE_SOUND_INDEX)), 0);
                    } else {
                        let play_type =
                            self.call(call::TEST_SPEECH, speaking_unit_index, m.i32(at(c::LINE_SOUND_INDEX))).result as i16;
                        if play_type == 1 {
                            busy = true;
                        } else if play_type > 0 {
                            self.call(call::SPEAK, conversation_index, play_type as i32);
                        }
                    }
                }
            }
            if !busy {
                m.set_bool(at(c::LINE_SPOKEN), true);
                m.set_bool(at(c::ANY_LINE_SPOKEN), true);
            }
        }

        if m.bool(at(c::LINE_SPOKEN)) {
            if !m.bool(at(c::LINE_FINISHED)) {
                let line_unit_index = m.i32(at(c::LINE_UNIT_INDEX));
                let finished = if line_unit_index == NONE {
                    let sound_index = m.i32(at(c::LINE_SOUND_INDEX));
                    sound_index == NONE || self.call(call::SCRIPTED_SOUND_TIME, sound_index, 0).result == 0
                } else {
                    let step = self.context.step_context();
                    let unit = object_get(m, &step, line_unit_index).unwrap_or(0);
                    m.i16(unit + u::UNIT_SPEECH_CURRENT_PRIORITY as u32) != PRIORITY_YELL
                };
                m.set_bool(at(c::LINE_FINISHED), finished);
            }

            if m.bool(at(c::LINE_FINISHED)) {
                let delay_timer = m.i16(at(c::LINE_DELAY_TIMER));
                if delay_timer > 0 {
                    m.set_i16(at(c::LINE_DELAY_TIMER), delay_timer - 1);
                    return m.bool(at(c::LINE_ADVANCE));
                }
                m.set_bool(at(c::LINE_ADVANCE), true);
                if flag(LINE_WAIT_AFTER_UNTIL_TOLD_TO_ADVANCE_BIT) {
                    if !m.bool(at(c::WAITING_TO_ADVANCE)) {
                        m.set_bool(at(c::WAITING_TO_ADVANCE), true);
                        m.set_bool(at(c::TOLD_TO_ADVANCE), false);
                    }
                    if m.bool(at(c::TOLD_TO_ADVANCE)) {
                        m.set_bool(at(c::WAITING_TO_ADVANCE), false);
                    } else {
                        m.set_bool(at(c::LINE_ADVANCE), false);
                    }
                }
            }
        }

        m.bool(at(c::LINE_ADVANCE))
    }

    /// One conversation's turn of the update.
    fn update(&self, conversation_index: i32, conversation: u32) {
        let m = self.m;
        let at = |offset: usize| conversation + offset as u32;
        let definition = self.definition(conversation);
        let line_count = tag_block_count(m, definition + ac::LINES as u32);

        if !m.bool(at(c::BEGUN)) {
            let mut continue_trying = true;
            if (self.context.game_time - m.i32(at(c::CREATION_TIME))) % 30 == 0 {
                continue_trying = self.call(call::BEGIN, conversation_index, 0).result2 != 0;
            }
            if !m.bool(at(c::BEGUN)) && !continue_trying {
                self.call(call::FINISH, conversation_index, 1);
            }
        }

        if m.bool(at(c::BEGUN)) && !m.bool(at(c::FINISHED)) {
            let line_index = m.i16(at(c::LINE_INDEX)) as i32;
            let mut perform = line_index >= 0 && line_index < line_count;
            while !perform || self.line_perform(conversation_index) {
                // (ai_conversation_line_end does nothing but find the
                // definition)
                let line_index = m.i16(at(c::LINE_INDEX)).wrapping_add(1);
                m.set_i16(at(c::LINE_INDEX), line_index);
                if (line_index as i32) < line_count {
                    perform = self.line_begin(conversation_index);
                } else {
                    m.set_bool(at(c::FINISHED), true);
                    break;
                }
            }
        }

        if m.bool(at(c::FINISHED)) {
            self.call(call::FINISH, conversation_index, 2);
        } else if m.bool(at(c::BEGUN)) {
            for participant_index in 0..self.participant_count(definition) {
                let actor_index = self.actor_index_of(conversation, participant_index);
                if m.r32(at(c::PARTICIPANT_BITMASK)) & (1 << participant_index) == 0 || actor_index == NONE {
                    continue;
                }
                let actor = self.actor(actor_index);
                let attention = actor + a::EXTERNAL_ORDERS_CONVERSATION_ATTENTION_UNIT_INDEX as u32;
                m.w32(actor + a::EXTERNAL_ORDERS_CONVERSATION_INDEX as u32, conversation_index as u32);
                let actor_unit_index = m.i32(actor + a::META_UNIT_INDEX as u32);
                m.w32(attention, NONE as u32);
                let speaker_unit_index = m.i32(at(c::LINE_UNIT_INDEX));
                let flag = |bit: u16| m.r16(at(c::LINE_FLAGS)) & (1 << bit) != 0;
                if actor_unit_index == speaker_unit_index {
                    m.w32(attention, m.r32(at(c::LINE_ADDRESS_UNIT_INDEX)));
                } else {
                    let address_unit_index = m.i32(at(c::LINE_ADDRESS_UNIT_INDEX));
                    if actor_unit_index == address_unit_index && flag(LINE_ADDRESSEE_LOOK_BACK_BIT) {
                        m.w32(attention, speaker_unit_index as u32);
                    } else if flag(LINE_EVERYONE_LOOK_AT_SPEAKER_BIT) {
                        m.w32(attention, speaker_unit_index as u32);
                    } else if flag(LINE_EVERYONE_LOOK_AT_ADDRESSEE_BIT) {
                        m.w32(attention, address_unit_index as u32);
                    }
                }
            }
        }
    }
}

/// `ai_conversation_update`.
pub fn ai_conversation_update(context: &WorldContext, host: &impl AiHost) {
    let conversations = Conversations { m: host.memory(), host, context };
    let m = conversations.m;
    let data = context.conversation_data;
    let mut absolute_index = 0i16;
    loop {
        let size = m.i16(data + DATA_SIZE) as i32;
        if absolute_index >= m.i16(data + DATA_COUNT) {
            break;
        }
        let header = m.r32(data + DATA_DATA).wrapping_add((size * absolute_index as i32) as u32);
        let identifier = m.i16(header);
        let datum_index = ((identifier as i32) << 16) | absolute_index as i32;
        absolute_index += 1;
        if identifier != 0 {
            conversations.update(datum_index, header);
        }
    }
}
