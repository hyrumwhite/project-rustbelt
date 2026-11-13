//! Telling a camera cut from fast motion, for `reset_history_on_cuts`.
//!
//! A cut drops the lighting's history (the reservoirs, the world cache, the
//! upscaler's past frames), so a false one blacks the cache and refills it.
//! A fixed distance in a frame (3 m) cut The Maw's intro over and over: its
//! camera flies 5 to 8 metres a game tick, and the viewer shows the game's
//! camera in steps (it repeats on the frames no game frame arrived), so a
//! step often passed 3 m in a steady flight. A cut is a jump out of line
//! with the camera's recent motion: a step larger than what the recent speed
//! would carry it in a few times the time since it last moved or in two game
//! ticks, whichever is longer, or than twice its longest recent step, and
//! never less than the fixed distance, so the first move after rest and a
//! jump from a walk are judged as before. (Measured on The Maw's intro to
//! about tick 1300: 96 cuts by the fixed distance, 6 by the speed alone, 11
//! with the longest step, of which 3 real; the two ticks' floor is for the
//! last 8.)

use std::collections::VecDeque;

use bevy::math::Vec3;

/// A camera that moved this far in a step (metres) cut, unless its recent
/// motion carries it further ([`CUT_SPEED_FACTOR`]); or turned this far
/// (radians): a cutscene's next shot, a respawn, a teleport, the free camera
/// taken or given back. Moving the fastest vehicle or turning the fastest
/// flick goes a small part of either in a frame.
pub const CUT_DISTANCE: f32 = 3.0;
pub const CUT_ANGLE: f32 = std::f32::consts::FRAC_PI_4;

/// How many times the step its recent speed would carry it a moving camera
/// may go without a cut (a game's frames come unevenly, a flight speeds up);
/// a shot changed in a fast flight is normally far larger and turns the
/// camera too.
pub const CUT_SPEED_FACTOR: f32 = 3.0;

/// The least time (seconds) the recent speed is allowed to carry the camera
/// in a step: two game ticks' travel. The viewer moves the camera every
/// frame between the game's ticks, and a tick's snapshot that lands off the
/// beat snaps it by up to a tick or so of its motion in one ordinary frame
/// (The Maw's intro: 5.6 to 10.8 m in a 5.8 ms frame at 240 to 270 m/s,
/// where its steps were 1.5 to 3 m), which neither the frame's time nor the
/// longest recent step foresees.
pub const CUT_SPEED_TIME: f32 = 2.0 / 30.0;

/// How many times its longest step of the last [`SPEED_WINDOW`] a moving
/// camera may step without a cut. The game's camera moves once a tick (30 a
/// second) while the viewer draws 60 or more frames, and a tick's snapshot
/// that comes early makes the time since the camera last moved short for a
/// whole step (The Maw's intro: 10 m steps cut against an allowance of 7 to
/// 9 by the speed); the steps themselves are steady, and a late snapshot
/// may carry two ticks' motion.
pub const CUT_STEP_FACTOR: f32 = 2.0;

/// How long (seconds) the moved frames are remembered: their distance over
/// their time is the recent speed, and the longest of their steps is the
/// recent step. (The speed is the window's mean, not its frames' largest:
/// a step over an early snapshot's 3 ms is thousands of metres a second,
/// and by it a 100 m jump in a 10 m-a-step flight passed.)
pub const SPEED_WINDOW: f32 = 0.5;

/// A step shorter than this (metres) is the camera standing still: the
/// game's camera repeated on a frame its frames skipped.
pub const MOVE_EPSILON: f32 = 1e-3;

/// The least time (seconds) a step is taken over, against a zero frame time.
const MIN_STEP_TIME: f32 = 1e-4;

/// The step after a cut that goes on as the cut's own step did, within this
/// ratio of length and [`AFTER_CUT_ANGLE`] of direction, is a flight carried
/// on, not another cut: a cut clears the speeds, and a fast flight shown
/// from its first frame (a cutscene's opening shot) would otherwise cut at
/// every step.
const AFTER_CUT_RATIO: f32 = 2.0;
const AFTER_CUT_ANGLE: f32 = std::f32::consts::FRAC_PI_6;

/// Which rule a cut fell to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutRule {
    /// Further than [`CUT_DISTANCE`] with no motion to carry it.
    Distance,
    /// Further than the recent motion carries it, by its speed or its
    /// longest step (and than [`CUT_DISTANCE`]).
    Speed,
    /// Turned more than [`CUT_ANGLE`].
    Angle,
}

/// One frame's step as judged.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    /// How far the camera went (metres) and turned (radians).
    pub distance: f32,
    pub angle: f32,
    /// How far it might have gone without a cut, the recent speed (m/s) and
    /// the longest recent step (metres) that allowed it, and the allowance
    /// each gave.
    pub allowed: f32,
    pub speed: f32,
    pub longest: f32,
    pub by_speed: f32,
    pub by_step: f32,
    /// Let through as the flight a cut's step began ([`AFTER_CUT_RATIO`]).
    pub carried_on: bool,
    /// The cut, if it was one.
    pub cut: Option<CutRule>,
}

/// The camera's recent motion and the cut rule over it.
#[derive(Default)]
pub struct CutDetector {
    /// The fixed distance alone, as before (`HALO_RT_CUT_DISTANCE_ONLY=1`).
    pub distance_only: bool,
    last: Option<(Vec3, Vec3)>,
    /// The time since the start, and since the camera last moved (seconds).
    clock: f32,
    since_moved: f32,
    /// The moved frames' (clock, step, time it took) within
    /// [`SPEED_WINDOW`], none of them cuts.
    moves: VecDeque<(f32, f32, f32)>,
    /// The last cut's step, until the next frame the camera moves.
    after_cut: Option<Vec3>,
}

impl CutDetector {
    pub fn new(distance_only: bool) -> Self {
        Self { distance_only, ..Self::default() }
    }

    /// Judges the camera's step to `position` looking along `forward`,
    /// `dt` seconds after the last frame; `None` on the first frame.
    pub fn step(&mut self, position: Vec3, forward: Vec3, dt: f32) -> Option<Step> {
        self.clock += dt.max(0.0);
        self.since_moved += dt.max(0.0);
        let Some((last_position, last_forward)) = self.last.replace((position, forward)) else {
            self.since_moved = 0.0;
            return None;
        };
        let moved = position - last_position;
        let distance = moved.length();
        let angle = last_forward.angle_between(forward);
        let since = self.since_moved.max(MIN_STEP_TIME);
        while self.moves.front().is_some_and(|&(at, ..)| at < self.clock - SPEED_WINDOW) {
            self.moves.pop_front();
        }
        let (moved_in_window, time_in_window) = self.moves.iter().fold((0.0, 0.0), |(d, t), &(_, step, took)| (d + step, t + took));
        let speed = if time_in_window > 0.0 { moved_in_window / time_in_window } else { 0.0 };
        let longest = self.moves.iter().map(|&(_, step, _)| step).fold(0.0, f32::max);
        let (by_speed, by_step) = (speed * (CUT_SPEED_FACTOR * since).max(CUT_SPEED_TIME), CUT_STEP_FACTOR * longest);
        let allowed = if self.distance_only { CUT_DISTANCE } else { CUT_DISTANCE.max(by_speed).max(by_step) };
        // (a flight carried on from the cut's own step)
        let carried_on = !self.distance_only
            && distance > MOVE_EPSILON
            && self.after_cut.is_some_and(|cut| {
                let ratio = distance / cut.length().max(MOVE_EPSILON);
                (1.0 / AFTER_CUT_RATIO..=AFTER_CUT_RATIO).contains(&ratio) && cut.angle_between(moved) < AFTER_CUT_ANGLE
            });
        let cut = if distance > allowed && !carried_on {
            Some(if allowed > CUT_DISTANCE { CutRule::Speed } else { CutRule::Distance })
        } else if angle > CUT_ANGLE {
            Some(CutRule::Angle)
        } else {
            None
        };
        if cut.is_some() {
            // (the cut's step is no motion of the flight's: the motion
            // before it is another shot's)
            self.moves.clear();
            self.after_cut = Some(moved);
            self.since_moved = 0.0;
        } else if distance > MOVE_EPSILON {
            self.moves.push_back((self.clock, distance, since));
            self.after_cut = None;
            self.since_moved = 0.0;
        }
        Some(Step { distance, angle, allowed, speed, longest, by_speed, by_step, carried_on, cut })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 120.0;

    /// Cuts in a flight of `steps` steps of `length` along x, the camera
    /// moving every other frame.
    fn fly(detector: &mut CutDetector, start: Vec3, length: f32, steps: usize) -> (Vec3, Vec<Option<CutRule>>) {
        let mut at = start;
        let mut cuts = Vec::new();
        for _ in 0..steps {
            at += Vec3::X * length;
            cuts.push(detector.step(at, Vec3::X, DT).and_then(|s| s.cut));
            // (the repeated frame)
            assert!(detector.step(at, Vec3::X, DT).and_then(|s| s.cut).is_none());
        }
        (at, cuts)
    }

    fn at_rest(detector: &mut CutDetector, at: Vec3, frames: usize) {
        for _ in 0..frames {
            assert!(detector.step(at, Vec3::X, DT).and_then(|s| s.cut).is_none());
        }
    }

    #[test]
    fn steady_fast_flight_cuts_only_at_its_start() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 120);
        let (_, cuts) = fly(&mut detector, Vec3::ZERO, 7.0, 60);
        assert_eq!(cuts[0], Some(CutRule::Distance));
        assert!(cuts[1..].iter().all(Option::is_none), "{cuts:?}");
    }

    #[test]
    fn steady_fast_flight_cuts_at_every_step_under_the_old_rule() {
        let mut detector = CutDetector::new(true);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (_, cuts) = fly(&mut detector, Vec3::ZERO, 7.0, 10);
        assert!(cuts.iter().all(|cut| *cut == Some(CutRule::Distance)));
    }

    #[test]
    fn first_step_from_rest_past_the_distance_cuts() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 120);
        let step = detector.step(Vec3::X * 3.5, Vec3::X, DT).unwrap();
        assert_eq!(step.cut, Some(CutRule::Distance));
    }

    #[test]
    fn a_jump_in_a_fast_flight_cuts() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (at, _) = fly(&mut detector, Vec3::ZERO, 7.0, 30);
        let step = detector.step(at + Vec3::new(0.0, 60.0, 0.0), Vec3::X, DT).unwrap();
        assert_eq!(step.cut, Some(CutRule::Speed));
        assert!(step.allowed > CUT_DISTANCE && step.allowed < 60.0);
    }

    #[test]
    fn a_turn_cuts() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let turned = Vec3::new(50f32.to_radians().cos(), 0.0, 50f32.to_radians().sin());
        let step = detector.step(Vec3::ZERO, turned, DT).unwrap();
        assert_eq!(step.cut, Some(CutRule::Angle));
    }

    #[test]
    fn a_teleport_from_a_walk_cuts() {
        let mut detector = CutDetector::new(false);
        let mut at = Vec3::ZERO;
        at_rest(&mut detector, at, 10);
        for _ in 0..60 {
            at += Vec3::X * 0.1;
            assert!(detector.step(at, Vec3::X, DT).unwrap().cut.is_none());
        }
        let step = detector.step(at + Vec3::Z * 4.0, Vec3::X, DT).unwrap();
        assert_eq!(step.cut, Some(CutRule::Distance));
    }

    /// A flight of `steps` steps of `length` along x, a step each frame at
    /// frame times taken in turn from `dts`; the cuts after the first two
    /// steps, and where it ends.
    fn jittered_flight(detector: &mut CutDetector, length: f32, steps: usize, dts: &[f32]) -> (Vec3, usize) {
        let mut at = Vec3::ZERO;
        let mut cuts = 0;
        for index in 0..steps {
            at += Vec3::X * length;
            let cut = detector.step(at, Vec3::X, dts[index % dts.len()]).and_then(|s| s.cut);
            if index >= 2 && cut.is_some() {
                cuts += 1;
            }
        }
        (at, cuts)
    }

    const JITTER: [f32; 3] = [0.003, 0.063, 0.033];

    #[test]
    fn a_jittered_steady_flight_does_not_cut() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (_, cuts) = jittered_flight(&mut detector, 10.0, 300, &JITTER);
        assert_eq!(cuts, 0);
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (_, cuts) = jittered_flight(&mut detector, 10.0, 300, &[0.003, 0.063]);
        assert_eq!(cuts, 0);
    }

    #[test]
    fn a_jump_in_a_jittered_flight_cuts() {
        for frame_time in JITTER {
            let mut detector = CutDetector::new(false);
            at_rest(&mut detector, Vec3::ZERO, 10);
            let (at, _) = jittered_flight(&mut detector, 10.0, 60, &JITTER);
            let step = detector.step(at + Vec3::new(0.0, 100.0, 0.0), Vec3::X, frame_time).unwrap();
            assert_eq!(step.cut, Some(CutRule::Speed), "{step:?}");
        }
    }

    #[test]
    fn a_turning_jump_in_a_jittered_flight_cuts() {
        // (2.5 steps, within the speed's allowance after a long frame: the
        // turn cuts it)
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (at, _) = jittered_flight(&mut detector, 10.0, 60, &JITTER);
        let turned = Vec3::new(60f32.to_radians().cos(), 0.0, 60f32.to_radians().sin());
        let step = detector.step(at + Vec3::X * 25.0, turned, 0.063).unwrap();
        assert_eq!(step.cut, Some(CutRule::Angle), "{step:?}");
    }

    /// A flight of 1.5 m steps every 5.8 ms (about 258 m/s), the camera
    /// snapping 10 m every 30th frame (a snapshot off the beat): the cuts
    /// after its first two steps, and where it ends.
    fn snapping_flight(detector: &mut CutDetector, steps: usize) -> (Vec3, usize) {
        let mut at = Vec3::ZERO;
        let mut cuts = 0;
        for index in 0..steps {
            at += Vec3::X * if index % 30 == 29 { 10.0 } else { 1.5 };
            let cut = detector.step(at, Vec3::X, 0.0058).and_then(|s| s.cut);
            if index >= 2 && cut.is_some() {
                cuts += 1;
            }
        }
        (at, cuts)
    }

    #[test]
    fn snaps_in_a_fast_flight_do_not_cut() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (_, cuts) = snapping_flight(&mut detector, 600);
        assert_eq!(cuts, 0);
    }

    #[test]
    fn a_jump_in_a_snapping_flight_cuts() {
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        let (at, _) = snapping_flight(&mut detector, 100);
        let step = detector.step(at + Vec3::new(0.0, 60.0, 0.0), Vec3::X, 0.0058).unwrap();
        assert_eq!(step.cut, Some(CutRule::Speed), "{step:?}");
    }

    #[test]
    fn a_second_jump_after_a_cut_cuts() {
        // (the cut's step allows nothing but a flight going on as it went)
        let mut detector = CutDetector::new(false);
        at_rest(&mut detector, Vec3::ZERO, 10);
        assert!(detector.step(Vec3::X * 60.0, Vec3::X, DT).unwrap().cut.is_some());
        let step = detector.step(Vec3::new(60.0, 60.0, 0.0), Vec3::X, DT).unwrap();
        assert!(step.cut.is_some());
    }
}
