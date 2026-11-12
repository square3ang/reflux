//! `NekoSprite` (`Accessibility.showNeko` / `keepNekoStill`): oneko's cat chases the pointer,
//! naps and scratches once it catches up, and can be petted, dragged and flung off the window's
//! edges. The sheet is Fluxer's (`assets/media/images/neko.png`). The poll steps it on the clock
//! and only asks to be woken when the cat next changes: every frame in flight, every 100 ms
//! while it walks, and at the next sprite change (or the next nap) while it rests.

use std::cell::Cell;
use std::collections::VecDeque;

use rune::{El, col, el, row, text};

use super::{BODY_KEY, ShellAccess, ShellState};
use crate::theme::{Palette, font};

/// A sheet cell: (column, row).
type Sprite = (u8, u8);

/// `SPRITE_SETS`, as cells.
fn sprites(name: &str) -> &'static [Sprite] {
    match name {
        "alert" => &[(7, 3)],
        "tired" => &[(3, 2)],
        "sleeping" => &[(2, 0), (2, 1)],
        "scratchSelf" => &[(5, 0), (6, 0), (7, 0)],
        "scratchWallN" => &[(0, 0), (0, 1)],
        "scratchWallS" => &[(7, 1), (6, 2)],
        "scratchWallE" => &[(2, 2), (2, 3)],
        "scratchWallW" => &[(4, 0), (4, 1)],
        "lookN" => &[(8, 2)],
        "lookS" => &[(8, 3)],
        "lookE" => &[(8, 1)],
        "lookW" => &[(8, 0)],
        "wallSquishSmall" => &[(9, 0)],
        "wallSquishLarge" => &[(9, 1)],
        "ceilingSquishSmall" => &[(9, 2)],
        "ceilingSquishLarge" => &[(9, 3)],
        "floorSquishSmall" => &[(10, 1)],
        "floorSquishLarge" => &[(10, 2)],
        "sulk" => &[(10, 2), (10, 3)],
        "N" => &[(1, 2), (1, 3)],
        "NE" => &[(0, 2), (0, 3)],
        "E" => &[(3, 0), (3, 1)],
        "SE" => &[(5, 1), (5, 2)],
        "S" => &[(6, 3), (7, 2)],
        "SW" => &[(5, 3), (6, 1)],
        "W" => &[(4, 2), (4, 3)],
        "NW" => &[(1, 0), (1, 1)],
        _ => &[(3, 3)],
    }
}

fn sprite(name: &str, frame: usize) -> Sprite {
    let set = sprites(name);
    set[frame % set.len()]
}

const CELL: f32 = 32.0;
const HALF: f32 = 16.0;
const SPEED: f32 = 10.0;
const FRAME_INTERVAL: f64 = 100.0;
const IDLE_DISTANCE: f32 = 48.0;
const DRAG_THRESHOLD: f32 = 6.0;
const FRICTION: f32 = 0.9;
const STOP_SPEED: f32 = 0.02;
const BOUNCE_FRICTION: f32 = 0.85;
const MIN_FLING_SPEED: f32 = 0.05;
const MAX_FLING_SPEED: f32 = 5.0;
const FLING_SAMPLE_WINDOW_MS: f64 = 100.0;
const FLING_STALE_MS: f64 = 80.0;
const MAX_FRAME_DT: f64 = 32.0;
const SULK_THRESHOLD: f32 = 0.7;
const HEAVY_SPEED: f32 = 1.5;
const SQUISH_WINDUP_MS: f32 = 120.0;
const SQUISH_SMALL_HOLD_MS: f64 = 200.0;
const SQUISH_LARGE_HOLD_MS: f64 = 350.0;
const SLOW_NEAR_MS: f32 = 90.0;
const SLOW_SCALE: f32 = 0.5;
const SLOW_EXIT_MS: f64 = 120.0;
/// `nekoHeartFloat` (1 s), or `nekoHeartFade` (0.8 s) under reduced motion.
const HEART_MS: f64 = 1000.0;
const HEART_REDUCED_MS: f64 = 800.0;

/// Fluxer's `clamp`: `min(max(v, lo), hi)`, so `hi` wins when they cross.
fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    v.max(lo).min(hi)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    Wall,
    Ceiling,
    Floor,
}

fn squish(surface: Surface, large: bool) -> &'static str {
    match (surface, large) {
        (Surface::Wall, false) => "wallSquishSmall",
        (Surface::Wall, true) => "wallSquishLarge",
        (Surface::Ceiling, false) => "ceilingSquishSmall",
        (Surface::Ceiling, true) => "ceilingSquishLarge",
        (Surface::Floor, false) => "floorSquishSmall",
        (Surface::Floor, true) => "floorSquishLarge",
    }
}

#[derive(Clone, Copy, Debug)]
struct Smash {
    surface: Surface,
    mirror: bool,
    heavy: bool,
    impact_at: Option<f64>,
}

#[derive(Clone, Debug)]
struct Queued {
    anim: &'static str,
    mirror: bool,
    hold: Option<usize>,
    left: u32,
    frame: usize,
}

#[derive(Clone, Debug)]
pub struct Neko {
    /// Centre in window points; `None` until shown (then near the bottom-right corner).
    pub at: Option<(f32, f32)>,
    /// Where the pointer last was; `None` is where the cat starts.
    mouse: Option<(f32, f32)>,
    sprite: Sprite,
    /// `scaleX(-1)`.
    mirror: bool,
    frame_count: usize,
    idle_time: u32,
    /// The `idle_time` the next nap or scratch starts at (drawn once instead of a 0.5 % roll
    /// every tick, so a resting cat needs no wakeups).
    idle_due: Option<u32>,
    idle_anim: Option<&'static str>,
    idle_frame: usize,
    queue: VecDeque<Queued>,
    /// A press on the cat: where (window points), and whether it has become a drag.
    press: Option<(f32, f32, bool)>,
    samples: Vec<(f32, f32, f64)>,
    /// Points per ms.
    vel: (f32, f32),
    flinging: bool,
    /// `didFlingRef`: after a fling the cat stays put until pressed again.
    flung: bool,
    smash: Option<Smash>,
    slow_release: Option<(f32, f64)>,
    bounced: bool,
    /// Clock times in ms: the last poll, the last 100 ms step, and when each heart spawned.
    last: Option<f64>,
    last_frame: f64,
    hearts: Vec<f64>,
    now: f64,
    /// The window body's size, as the view last laid it out.
    bounds: Cell<[f32; 2]>,
}

impl Default for Neko {
    fn default() -> Self {
        Self {
            at: None,
            mouse: None,
            sprite: sprite("idle", 0),
            mirror: false,
            frame_count: 0,
            idle_time: 0,
            idle_due: None,
            idle_anim: None,
            idle_frame: 0,
            queue: VecDeque::new(),
            press: None,
            samples: Vec::new(),
            vel: (0.0, 0.0),
            flinging: false,
            flung: false,
            smash: None,
            slow_release: None,
            bounced: false,
            last: None,
            last_frame: f64::NEG_INFINITY,
            hearts: Vec::new(),
            now: 0.0,
            bounds: Cell::new([1280.0, 800.0]),
        }
    }
}

fn start([w, h]: [f32; 2]) -> (f32, f32) {
    (clamp(w - 48.0, HALF, w - HALF), clamp(h - 96.0, HALF, h - HALF))
}

/// The sprite an idle animation draws at `frame`, `None` past its end.
fn idle_sprite(anim: &str, frame: usize) -> Option<Sprite> {
    match anim {
        "sleeping" if frame > 193 => None,
        "sleeping" if frame < 8 => Some(sprite("tired", 0)),
        "sleeping" => Some(sprite("sleeping", frame / 4)),
        _ if frame > 10 => None,
        _ => Some(sprite(anim, frame)),
    }
}

/// Ticks until a 0.5 %-per-tick roll first succeeds (0 = this one).
fn geometric() -> u32 {
    ((1.0 - fastrand::f64()).ln() / (1.0f64 - 0.005).ln()).min(1e6) as u32
}

/// `reflectAxis`: (position, velocity, bounced, hit the low end).
fn reflect(pos: f32, vel: f32, dt: f32, lo: f32, hi: f32) -> (f32, f32, bool, bool) {
    let next = pos + vel * dt;
    if next < lo {
        (clamp(lo + (lo - next) * BOUNCE_FRICTION, lo, hi), -vel * BOUNCE_FRICTION, true, true)
    } else if next > hi {
        (clamp(hi - (next - hi) * BOUNCE_FRICTION, lo, hi), -vel * BOUNCE_FRICTION, true, false)
    } else {
        (next, vel, false, false)
    }
}

/// `timeToBoundMs` under the fling's friction.
fn time_to_bound(pos: f32, vel: f32, lo: f32, hi: f32) -> f32 {
    let decay = FRICTION.ln() / 16.67;
    if vel == 0.0 {
        return f32::INFINITY;
    }
    let distance = if vel > 0.0 { hi - pos } else { pos - lo };
    let reach = 1.0 + distance * decay / vel.abs();
    if reach <= 0.0 { f32::INFINITY } else { reach.ln() / decay }
}

impl Neko {
    fn show(&mut self, name: &str, frame: usize) {
        self.sprite = sprite(name, frame);
    }

    fn reset_idle(&mut self) {
        (self.idle_anim, self.idle_frame, self.idle_due) = (None, 0, None);
    }

    fn dragging(&self) -> bool {
        self.press.is_some_and(|p| p.2)
    }

    fn distance(&self) -> f32 {
        let (x, y) = self.at.unwrap_or_default();
        let (mx, my) = self.mouse.unwrap_or((x, y));
        (x - mx).hypot(y - my)
    }

    /// One 100 ms step at rest, `n` steps after the last (a resting cat sleeps through the
    /// steps where nothing changes).
    fn idle(&mut self, n: u32, [w, h]: [f32; 2]) {
        self.idle_time += n;
        if self.idle_anim.is_some() {
            self.idle_frame += n as usize - 1;
        } else {
            let due = *self.idle_due.get_or_insert_with(|| self.idle_time.max(11) + geometric());
            if self.idle_time < due {
                self.show("idle", 0);
                return;
            }
            self.idle_due = None;
            let (x, y) = self.at.unwrap_or_default();
            let mut available = vec!["sleeping", "scratchSelf"];
            if x < CELL {
                available.push("scratchWallW");
            }
            if y < CELL {
                available.push("scratchWallN");
            }
            if x > w - CELL {
                available.push("scratchWallE");
            }
            if y > h - CELL {
                available.push("scratchWallS");
            }
            self.idle_anim = Some(available[fastrand::usize(..available.len())]);
            self.idle_frame += (self.idle_time - due) as usize;
        }
        let anim = self.idle_anim.unwrap_or("idle");
        self.sprite = idle_sprite(anim, self.idle_frame).unwrap_or(sprite("idle", 0));
        if idle_sprite(anim, self.idle_frame + 1).is_none() {
            self.reset_idle();
        }
        // As Fluxer's, this counts on past a reset, so later animations start a frame in.
        self.idle_frame += 1;
    }

    /// `step`: chase the pointer, or rest near it.
    fn step(&mut self, n: u32, still: bool, bounds: [f32; 2]) {
        if still {
            return self.idle(n, bounds);
        }
        self.frame_count += 1;
        let (x, y) = self.at.unwrap_or_default();
        let (mx, my) = self.mouse.unwrap_or((x, y));
        let (dx, dy) = (x - mx, y - my);
        let distance = dx.hypot(dy);
        if distance < SPEED || distance < IDLE_DISTANCE {
            return self.idle(n, bounds);
        }
        self.reset_idle();
        if self.idle_time > 1 {
            self.show("alert", 0);
            self.idle_time = self.idle_time.min(7) - 1;
            return;
        }
        let mut dir = String::new();
        dir += if dy / distance > 0.5 { "N" } else { "" };
        dir += if dy / distance < -0.5 { "S" } else { "" };
        dir += if dx / distance > 0.5 { "W" } else { "" };
        dir += if dx / distance < -0.5 { "E" } else { "" };
        self.show(&dir, self.frame_count);
        let [w, h] = bounds;
        let nx = clamp(x - dx / distance * SPEED, HALF, w - HALF);
        let ny = clamp(y - dy / distance * SPEED, HALF, h - HALF);
        self.at = Some((nx, ny));
    }

    fn play_queued(&mut self) -> bool {
        let Some(q) = self.queue.front_mut() else { return false };
        (self.sprite, self.mirror) = (sprite(q.anim, q.hold.unwrap_or(q.frame)), q.mirror);
        q.frame += 1;
        q.left -= 1;
        if q.left == 0 {
            self.queue.pop_front();
            if self.queue.is_empty() {
                self.mirror = false;
            }
        }
        true
    }

    fn flying(&mut self) {
        let (vx, vy) = self.vel;
        let dir = if vx.abs() > vy.abs() {
            if vx > 0.0 { "lookE" } else { "lookW" }
        } else if vy > 0.0 {
            "lookS"
        } else {
            "lookN"
        };
        self.mirror = false;
        self.show(dir, 0);
    }

    fn draw_smash(&mut self, now: f64) -> bool {
        let Some(s) = self.smash else { return false };
        let large = match s.impact_at {
            None => false,
            Some(at) => {
                if now - at >= if s.heavy { SQUISH_LARGE_HOLD_MS } else { SQUISH_SMALL_HOLD_MS } {
                    self.smash = None;
                    return false;
                }
                s.heavy
            }
        };
        self.show(squish(s.surface, large), 0);
        self.mirror = s.mirror;
        true
    }

    /// `updateMomentum`: one frame of a fling, slowing near a wall and squishing on it.
    fn momentum(&mut self, now: f64, dt: f64, [w, h]: [f32; 2]) {
        let frame_dt = dt.min(MAX_FRAME_DT) as f32;
        let (max_x, max_y) = (HALF.max(w - HALF), HALF.max(h - HALF));
        let (x, y) = self.at.unwrap_or_default();
        let (vx, vy) = self.vel;
        let speed = vx.hypot(vy);
        let (tx, ty) = (time_to_bound(x, vx, HALF, max_x), time_to_bound(y, vy, HALF, max_y));
        let to_wall = tx.min(ty);
        let hit = to_wall.is_finite().then_some(if tx <= ty { (Surface::Wall, vx < 0.0) } else if vy < 0.0 { (Surface::Ceiling, false) } else { (Surface::Floor, false) });
        let mut scale = 1.0;
        if let Some((from, at)) = self.slow_release {
            let progress = ((now - at) / SLOW_EXIT_MS).clamp(0.0, 1.0) as f32;
            scale = from + (1.0 - from) * progress;
            if progress >= 1.0 {
                self.slow_release = None;
            }
        }
        // `approachTimeScale`.
        if to_wall.is_finite() && to_wall < SLOW_NEAR_MS {
            scale = f32::min(scale, SLOW_SCALE + (1.0 - SLOW_SCALE) * to_wall.max(0.0) / SLOW_NEAR_MS);
        }
        let sim = frame_dt * scale;
        if self.smash.is_none()
            && speed >= HEAVY_SPEED
            && let Some((surface, mirror)) = hit
            && to_wall / scale <= SQUISH_WINDUP_MS
        {
            self.smash = Some(Smash { surface, mirror, heavy: true, impact_at: None });
        }
        let decay = FRICTION.powf(sim / 16.67);
        let (vx, vy) = (vx * decay, vy * decay);
        let (ivx, ivy) = (vx.abs(), vy.abs());
        let (nx, nvx, bx, x_min) = reflect(x, vx, sim, HALF, max_x);
        let (ny, nvy, by, y_min) = reflect(y, vy, sim, HALF, max_y);
        self.at = Some((nx, ny));
        self.vel = (nvx, nvy);
        if bx || by {
            self.bounced = true;
            if scale < 1.0 {
                self.slow_release = Some((scale, now));
            }
            let on_wall = bx && (!by || ivx >= ivy);
            let surface = if on_wall { Surface::Wall } else if y_min { Surface::Ceiling } else { Surface::Floor };
            let mirror = on_wall && x_min;
            if let Some(s) = self.smash.as_mut() {
                (s.impact_at, s.surface, s.mirror) = (Some(now), surface, mirror);
            } else {
                self.smash = Some(Smash { surface, mirror, heavy: ivx.hypot(ivy) >= HEAVY_SPEED, impact_at: Some(now) });
            }
        }
        if nvx.hypot(nvy) < STOP_SPEED {
            (self.flinging, self.vel, self.flung, self.smash) = (false, (0.0, 0.0), true, None);
            // `runAnimationCycle(sulk, SULK_NEKO_ANIMATION_THRESHOLD)`.
            if self.bounced && fastrand::f32() <= SULK_THRESHOLD {
                self.queue.push_back(Queued { anim: "sulk", mirror: false, hold: Some(0), left: 4, frame: 0 });
                self.queue.push_back(Queued { anim: "sulk", mirror: false, hold: Some(1), left: 10 + fastrand::u32(..16), frame: 0 });
            }
            return;
        }
        if !self.draw_smash(now) {
            self.flying();
        }
    }

    /// `tick`: a frame at `now` (ms).
    fn tick(&mut self, now: f64, still: bool, reduced: bool, bounds: [f32; 2]) {
        let first = self.last.is_none();
        let dt = self.last.map_or(16.67, |l| now - l);
        self.last = Some(now);
        if self.flinging {
            self.momentum(now, dt, bounds);
        }
        if now - self.last_frame < FRAME_INTERVAL {
            return;
        }
        let n = if first { 1 } else { ((now - self.last_frame) / FRAME_INTERVAL).clamp(1.0, 256.0) as u32 };
        self.last_frame = now;
        if self.play_queued() {
        } else if self.dragging() {
            self.show("alert", 0);
        } else if self.flinging {
            if self.smash.is_none() {
                self.flying();
            }
        } else if reduced {
            self.show("idle", 0);
        } else if self.flung {
            self.idle(n, bounds);
        } else {
            self.step(n, still, bounds);
        }
    }

    /// When the cat next changes by itself (ms), if it does.
    fn next_wake(&self, still: bool, reduced: bool) -> Option<f64> {
        if self.flinging || !self.hearts.is_empty() {
            return Some(self.now);
        }
        let ticks = if !self.queue.is_empty() {
            1
        } else if self.dragging() || reduced {
            return None;
        } else if let Some(anim) = self.idle_anim {
            (self.idle_frame..).position(|f| idle_sprite(anim, f) != Some(self.sprite)).unwrap_or(0) + 1
        } else if !still && !self.flung && self.distance() >= IDLE_DISTANCE.max(SPEED) {
            1
        } else {
            self.idle_due.map_or(1, |due| due.saturating_sub(self.idle_time).max(1)) as usize
        };
        Some(self.last_frame + ticks as f64 * FRAME_INTERVAL)
    }

    fn pet(&mut self, now: f64) {
        self.idle_time = 0;
        self.reset_idle();
        self.show("alert", 0);
        self.hearts.push(now);
    }

    /// `computeVelocity` over the last 100 ms of the drag, capped at `MAX_FLING_SPEED`.
    fn fling_velocity(&self, now: f64) -> (f32, f32, f32) {
        let recent: Vec<_> = self.samples.iter().filter(|s| now - s.2 <= FLING_SAMPLE_WINDOW_MS).collect();
        let (Some(a), Some(b)) = (recent.first(), recent.last()) else { return (0.0, 0.0, 0.0) };
        let dt = (b.2 - a.2) as f32;
        if recent.len() < 2 || dt <= 0.0 {
            return (0.0, 0.0, 0.0);
        }
        let (vx, vy) = ((b.0 - a.0) / dt, (b.1 - a.1) / dt);
        let scalar = vx.hypot(vy);
        if scalar > MAX_FLING_SPEED {
            let k = MAX_FLING_SPEED / scalar;
            return (vx * k, vy * k, MAX_FLING_SPEED);
        }
        (vx, vy, scalar)
    }

    fn drag(&mut self, d: &rune::Drag, now: f64) {
        let [px, py] = d.window_point();
        let [w, h] = self.bounds.get();
        match d.phase {
            rune::Phase::Down => {
                (self.flinging, self.flung, self.vel, self.smash, self.slow_release, self.bounced, self.mirror) = (false, false, (0.0, 0.0), None, None, false, false);
                self.samples.clear();
                self.queue.clear();
                self.press = Some((px, py, false));
            }
            rune::Phase::Move => {
                let Some((x0, y0, dragging)) = self.press else { return };
                if !dragging {
                    if (px - x0).hypot(py - y0) <= DRAG_THRESHOLD {
                        return;
                    }
                    self.press = Some((x0, y0, true));
                    self.reset_idle();
                    self.idle_time = 0;
                }
                let at = (clamp(px, HALF, w - HALF), clamp(py, HALF, h - HALF));
                (self.at, self.mouse) = (Some(at), Some(at));
                self.samples.push((px, py, now));
                if self.samples.len() > 5 {
                    self.samples.remove(0);
                }
            }
            rune::Phase::Up => {
                let dragged = self.dragging();
                if !d.cancelled() && !dragged {
                    self.pet(now);
                }
                let (vx, vy, scalar) = if dragged { self.fling_velocity(now) } else { (0.0, 0.0, 0.0) };
                let stale = self.samples.last().is_none_or(|s| now - s.2 > FLING_STALE_MS);
                let no_fling = crate::look::reduced_motion() || stale;
                self.vel = if no_fling { (0.0, 0.0) } else { (vx, vy) };
                self.flinging = !no_fling && scalar > MIN_FLING_SPEED;
                (self.smash, self.slow_release, self.bounced, self.press) = (None, None, false, None);
                self.samples.clear();
            }
        }
    }
}

/// The pointer moved over the window (`pointermove` on the document).
pub fn track(st: &mut ShellState, x: f32, y: f32) {
    if crate::look::with(|l| l.show_neko) && !st.neko.dragging() {
        st.neko.mouse = Some((x, y));
    }
}

/// Once a frame: steps the cat and asks for a wake when it next changes.
pub fn poll(st: &mut ShellState, tx: &mut rune::Tx) -> bool {
    let (show, still) = crate::look::with(|l| (l.show_neko, l.keep_neko_still));
    if !show {
        // Hidden, the engine unmounts; shown again it starts afresh.
        if st.neko.last.is_some() {
            st.neko = Neko::default();
        }
        return false;
    }
    if st.neko.last.is_none() {
        // Every cell up front, so no frame shows the cat blank while one decodes.
        for c in 0..11 {
            for r in 0..4 {
                let _ = st.media.neko((c, r), false);
            }
        }
        let _ = (st.media.neko((9, 0), true), st.media.neko((9, 1), true));
    }
    let n = &mut st.neko;
    let now = tx.clock() * 1000.0;
    let reduced = crate::look::reduced_motion();
    let bounds @ [w, h] = n.bounds.get();
    let before = (n.at, n.sprite, n.mirror, n.hearts.len());
    let (x, y) = n.at.unwrap_or_else(|| start(bounds));
    n.at = Some((clamp(x, HALF, w - HALF), clamp(y, HALF, h - HALF)));
    n.tick(now, still, reduced, bounds);
    n.now = now;
    let life = if reduced { HEART_REDUCED_MS } else { HEART_MS };
    n.hearts.retain(|t| now - t < life);
    if let Some(due) = n.next_wake(still, reduced) {
        tx.wake_at(due / 1000.0);
    }
    before != (n.at, n.sprite, n.mirror, n.hearts.len()) || !n.hearts.is_empty()
}

/// The cat overlay, above everything.
pub fn view<S: 'static>(st: &ShellState, _p: &Palette, acc: ShellAccess<S>) -> Option<El> {
    if !crate::look::with(|l| l.show_neko) {
        return None;
    }
    let n = &st.neko;
    if let Some(b) = rune::memo::layout_size(BODY_KEY) {
        n.bounds.set(b);
    }
    let (x, y) = n.at.unwrap_or_else(|| start(n.bounds.get()));
    let glyph = match st.media.neko(n.sprite, n.mirror) {
        crate::media::Load::Ready(px) => el().size(CELL, CELL).pixels_fit(&px, rune::SurfaceFit::Fit),
        _ => el().size(CELL, CELL),
    };
    let reduced = crate::look::reduced_motion();
    let ease = |t: f64| rune::Easing::EASE_OUT.eval(t as f32);
    let heart_color = rune::color::hex("#ff5c8a").unwrap_or(rune::Color::WHITE);
    // `.heart`: 12px, centred over the cat 4 above it, floating up 24 and growing (`nekoHeartFloat`)
    // or only fading (`nekoHeartFade`); ease-out per keyframe span.
    let hearts = n.hearts.iter().map(|t| {
        let p = (n.now - t) / if reduced { HEART_REDUCED_MS } else { HEART_MS };
        let o = if p < 0.2 { ease(p / 0.2) } else { 1.0 - ease((p - 0.2) / 0.8) };
        let e = if reduced { 0.0 } else { ease(p) };
        row()
            .absolute(0.0, -4.0 - 24.0 * e)
            .w(CELL)
            .justify_center()
            .inert()
            .child(text("♥").font(font(12.0, 400.0).lh(12.0)).color(heart_color).scale(if reduced { 1.0 } else { 0.6 + 0.5 * e }).scale_motion(rune::Curve::Instant))
            .opacity(o)
            .opacity_motion(rune::Curve::Instant)
    });
    Some(
        col()
            .absolute(x - HALF, y - HALF)
            .size(CELL, CELL)
            .motion(rune::Spring::NONE)
            .cursor(if n.press.is_some() { rune::Cursor::Grabbing } else { rune::Cursor::Grab })
            .label("Neko")
            .child(glyph)
            .children(hearts)
            .on_drag(move |s: &mut S, d: &mut rune::Drag, tx: &mut rune::Tx| acc(s).neko.drag(d, tx.clock() * 1000.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: [f32; 2] = [1280.0, 800.0];

    #[test]
    fn walks_toward_the_pointer_then_rests() {
        let mut n = Neko { at: Some((100.0, 100.0)), mouse: Some((300.0, 100.0)), last_frame: 0.0, ..Default::default() };
        n.step(1, false, B);
        assert_eq!(n.at, Some((110.0, 100.0)));
        assert!(sprites("E").contains(&n.sprite));
        assert_eq!(n.next_wake(false, false), Some(n.last_frame + FRAME_INTERVAL), "walking steps every 100 ms");
        for _ in 0..20 {
            n.step(1, false, B);
        }
        assert!(n.distance() < IDLE_DISTANCE, "stops near the pointer");
        assert_eq!(n.sprite, sprite("idle", 0));
        let due = n.idle_due.expect("the next nap is drawn");
        assert!(due >= 11);
        assert_eq!(n.next_wake(false, false), Some(n.last_frame + (due - n.idle_time).max(1) as f64 * FRAME_INTERVAL), "rests until it");
    }

    #[test]
    fn naps_on_the_due_tick_and_wakes_only_on_sprite_changes() {
        let mut n = Neko { at: Some((600.0, 400.0)), idle_time: 20, idle_due: Some(21), last_frame: 0.0, ..Default::default() };
        n.idle(1, B);
        let anim = n.idle_anim.expect("an idle animation starts");
        if anim == "sleeping" {
            assert_eq!(n.sprite, sprite("tired", 0));
            assert_eq!(n.next_wake(false, false), Some(8.0 * FRAME_INTERVAL), "tired until frame 8");
        }
        // Lumped ticks run it to its end.
        n.idle(400, B);
        assert_eq!(n.idle_anim, None);
        assert_eq!(n.sprite, sprite("idle", 0));
    }

    #[test]
    fn a_fling_bounces_off_the_wall_and_stops() {
        let mut n = Neko { at: Some((1200.0, 400.0)), vel: (3.0, 0.0), flinging: true, ..Default::default() };
        let mut t = 0.0;
        while n.flinging && t < 10_000.0 {
            t += 16.0;
            n.momentum(t, 16.0, B);
        }
        assert!(!n.flinging && n.flung && n.bounced);
        let (x, _) = n.at.unwrap();
        assert!((HALF..=B[0] - HALF).contains(&x));
    }
}
