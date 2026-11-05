//! A mod's hold on the world (`render.hold-world`, capability `render`): the terrain goes on being drawn as it stands while
//! what changes under it is worked out, and all of it takes its place on one frame. Finished sections are uploaded to the
//! arena as ever, over as many frames as they come, but keep to one side (`Waiting`) while the sections they replace stay
//! drawn; letting go swaps slots, with nothing left to upload, so a mod can change a great deal of land in the background
//! and show it at a stroke.
//!
//! Both the old and the new mesh of a section are in memory while it waits, so a hold costs up to what the changed land's
//! new meshes come to. It is bounded three ways, and each lets go at once rather than losing anything: `MAX_BYTES` of
//! waiting meshes, `MAX_HOLD` of time, and the arena running out (`Renderer::upload`). It never outlives what asked for it:
//! the game asks the mods every frame, a mod that stops or clears its pass holds nothing, and the sections' being cleared
//! (the world left, another dimension) drops what waits. A hold that ended by itself (`spent`) stays ended until the mod has
//! let go.
use std::time::{Duration, Instant};

/// The longest a hold lasts.
pub(super) const MAX_HOLD: Duration = Duration::from_secs(30);
/// The new meshes uploaded a frame while held: nothing shows for them, so they spread out over the frames (what is left
/// when the hold lets go goes up on the next frame).
pub const HELD_UPLOAD_BYTES: u64 = 2 << 20;
/// The most bytes of new meshes that wait.
pub(super) const MAX_BYTES: u64 = 1 << 30;

/// Whether the world is held, and what is waiting.
#[derive(Default)]
pub(super) struct Gate {
    since: Option<Instant>,
    spent: bool,
    bytes: u64,
}
impl Gate {
    /// Whether new sections wait.
    pub fn held(&self) -> bool {
        self.since.is_some() && !self.spent
    }
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
    /// A frame: whether the mods ask for the hold. True when what waits should take its place now.
    pub fn ask(&mut self, on: bool, now: Instant) -> bool {
        if !on {
            let release = self.held();
            *self = Self::default();
            return release;
        }
        if self.spent {
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if now.duration_since(since) >= MAX_HOLD {
            log::warn!("Horizon: a mod held the world for {} s: let go", MAX_HOLD.as_secs());
            self.spend();
            return true;
        }
        false
    }
    /// Ended by itself: nothing waits from now on, until the mod has let go.
    pub fn spend(&mut self) {
        self.spent = self.since.is_some();
        self.bytes = 0;
    }
    /// `bytes` more wait: true when that is too many.
    pub fn add(&mut self, bytes: u64) -> bool {
        self.bytes += bytes;
        self.bytes > MAX_BYTES
    }
    /// `bytes` wait no longer.
    pub fn take(&mut self, bytes: u64) {
        self.bytes = self.bytes.saturating_sub(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_starts_and_lets_go_when_asked() {
        let (mut g, t) = (Gate::default(), Instant::now());
        assert!(!g.ask(false, t) && !g.held());
        assert!(!g.ask(true, t) && g.held());
        assert!(!g.ask(true, t + Duration::from_secs(5)) && g.held());
        assert!(g.ask(false, t + Duration::from_secs(6)) && !g.held());
        assert!(!g.ask(false, t + Duration::from_secs(7)));
    }
    #[test]
    fn a_hold_ends_by_itself_and_stays_ended_until_let_go() {
        let (mut g, t) = (Gate::default(), Instant::now());
        g.ask(true, t);
        assert!(g.ask(true, t + MAX_HOLD) && !g.held());
        assert!(!g.ask(true, t + MAX_HOLD * 2) && !g.held());
        assert!(!g.ask(false, t + MAX_HOLD * 2));
        let later = t + MAX_HOLD * 3;
        assert!(!g.ask(true, later) && g.held());
    }
    #[test]
    fn what_waits_is_capped() {
        let (mut g, t) = (Gate::default(), Instant::now());
        g.ask(true, t);
        assert!(!g.add(MAX_BYTES));
        g.take(10);
        assert!(!g.add(10));
        assert!(g.add(1));
        g.spend();
        assert!(!g.held());
        assert_eq!(g.bytes(), 0);
        // (sections cleared with a hold on: as if spent, and no hold to end when there was none)
        let mut none = Gate::default();
        none.spend();
        assert!(!none.ask(true, t) && none.held());
    }
}
