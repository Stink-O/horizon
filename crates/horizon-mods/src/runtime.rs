//! Loading and calling mods with wasmtime's component model.
use crate::manifest::{Manifest, Package, Side};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, HasSelf, Linker},
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-mod",
        additional_derives: [PartialEq],
    });
}
pub use bindings::horizon::mods::{
    game::{Clock, Player},
    hud::{Corner, Line},
    input::{Click, Event as InputEvent, HeldItem, ItemUse, Typed},
    ui::{Draw, Point, Rect},
    world::{BlockEdit, BlockRun, ColumnBlocks, Heightmap, Hit, SurfaceHeights},
};
use bindings::{
    HorizonMod,
    exports::horizon::mods::events::Registration,
    horizon::mods::{chat, game, hud, input, log as mod_log, render, storage, ui, world},
};

/// A mod's linear memory cap.
const MEMORY: usize = 64 << 20;
/// Fuel (about one unit per WebAssembly instruction) per call: a bound on
/// runaway loops, far above the hooks' time budgets.
const FUEL_INIT: u64 = 2_000_000_000;
const FUEL_TICK: u64 = 50_000_000;
const FUEL_FRAME: u64 = 10_000_000;
const FUEL_EVENT: u64 = 100_000_000;
/// A call that uses this fraction of its fuel or more (1/2) is logged: a mod near being stopped.
const FUEL_WARN: u64 = 2;
/// Chat lines a mod may show per call, HUD lines and their length.
const CHAT_PER_CALL: usize = 8;
const HUD_LINES: usize = 16;
const LINE_CHARS: usize = 256;
/// Total bytes a mod's `storage` may hold on disk, across all its keys.
const STORAGE_QUOTA: u64 = 4 << 20;
/// Distinct keys a mod's `storage` may hold: `STORAGE_QUOTA` alone doesn't
/// bound a flood of zero- or near-zero-byte files (each still a file name,
/// a directory entry and an inode).
const STORAGE_MAX_KEYS: usize = 64;
/// Bytes `storage::set` may write across one hook call: fuel bounds a
/// mod's CPU time per call, not the time a synchronous disk write takes, so
/// a cheap-in-fuel loop replacing one key with large payloads needs its own
/// cap, separate from the on-disk quota it would otherwise stay under.
const STORAGE_WRITE_BUDGET: u64 = 1 << 20;
/// Input events kept for a mod between `poll`s; older ones are dropped.
const EVENTS_KEPT: usize = 256;
/// Items one mod may claim, keys it may register.
const CLAIMS: usize = 64;
const KEYS: usize = 32;
/// Draw operations in a mod's screen.
const UI_DRAWS: usize = 4096;
/// Commands one call may queue (`world.run-commands`), sounds per call.
const COMMANDS_PER_CALL: usize = 8192;
const SOUNDS_PER_CALL: usize = 64;
const SHAKES_PER_CALL: usize = 16;
/// `world.edit-blocks`: runs, and palette entries and filters, one call may make.
pub const EDIT_RUNS: usize = 1 << 20;
pub const EDIT_PALETTE: usize = 4096;
/// A post pass's source, and its parameters.
const POST_SOURCE: usize = 256 << 10;
pub const POST_PARAMS: usize = 16;
/// `render.set-data`: the words one call may give the pass (1 MiB).
pub const POST_DATA_WORDS: usize = 1 << 18;
/// `world.surface-height`: the columns one query may ask about, and the answers kept for a mod that does not poll
/// them (the oldest are dropped).
pub const SURFACE_COLUMNS: usize = 1 << 16;
const SURFACE_KEPT: usize = 256;
/// `world.read-columns`: the columns, the blocks a column and the blocks in all one query may ask for, and the answers
/// kept for a mod that does not poll them.
pub const COLUMNS_MAX: usize = 1 << 16;
pub const COLUMN_BLOCKS: u32 = 64;
pub const COLUMNS_BLOCKS_MAX: usize = 1 << 20;
const COLUMNS_KEPT: usize = 16;
/// How far `world.raycast` sees: through the loaded chunks, then across the distant terrain (LOD Distance).
pub const RAYCAST_MAX: f32 = 1024.0;
/// The fastest flying speed `world.set-flying-speed` gives (vanilla's is 0.05).
pub const FLYING_SPEED_MAX: f32 = 50.0;

/// What mods may read of the game, copied once per tick by the game.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The stack in the main hand (capability `input`).
    pub held: Option<HeldItem>,
    /// The first block along the view within `RAYCAST_MAX`, and its
    /// distance (capability `world`).
    pub view_hit: Option<(f32, Hit)>,
}

/// What mods asked the game to show since the last [`Mods::drain`].
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// `chat.show`: a line after the mod's name.
    Chat { name: String, text: String },
    /// A mod command's feedback, red when `error`.
    Feedback { text: String, error: bool },
    /// The mod was turned off: it trapped, overran or failed to start.
    Stopped { name: String, reason: String },
    /// `ui.show` opened mod `id`'s screen, or `ui.close` closed it.
    Screen { id: String, open: bool },
    /// `world.run-commands`: to run at operator level, silently, in order.
    Commands { id: String, commands: Vec<String> },
    /// `world.edit-blocks`: block changes to make at once, in order with the commands.
    Edit { id: String, edit: BlockEdit },
    /// `world.surface-height`: the heights of `columns` on `heightmap` to read, in order with the commands and the
    /// edits, and give back to mod `id` with [`Mods::push_surface`] (with `tag`).
    Surface {
        id: String,
        tag: u32,
        columns: Vec<(i32, i32)>,
        heightmap: Heightmap,
    },
    /// `world.read-columns`: the blocks of `columns` round their heights on `heightmap` (`below` under, `above` from
    /// it up) to read, in order with the commands and the edits, and give back to mod `id` with
    /// [`Mods::push_columns`] (with `tag`).
    Columns {
        id: String,
        tag: u32,
        columns: Vec<(i32, i32)>,
        heightmap: Heightmap,
        below: u32,
        above: u32,
    },
    /// `input.register-key`: the game lists the mods' keybinds again.
    Keys,
    /// `world.play-sound`.
    Sound {
        event: String,
        at: Option<[f64; 3]>,
        volume: f32,
        pitch: f32,
    },
    /// `render.shake`: a jolt of the camera, `strength` 0 to 1, fading out over `seconds`.
    Shake { strength: f32, seconds: f32 },
}

/// A keybind a mod registered (capability `input`).
#[derive(Clone, Debug, PartialEq)]
pub struct ModKey {
    pub mod_id: String,
    pub mod_name: String,
    /// The mod's own name for it.
    pub name: String,
    pub label: String,
    /// The default key, as options.txt writes keys.
    pub key: String,
}

/// A mod's post-processing pass (capability `render`).
#[derive(Clone, Debug)]
pub struct Post {
    pub source: Arc<str>,
    /// Changes when the source does: the game recompiles then.
    pub version: u64,
    pub params: [f32; POST_PARAMS],
    /// `render.set-data`'s words, and a version that changes with them (none: never set, or cleared).
    pub data: Option<(Arc<[u32]>, u64)>,
}

/// A mod's text panel, drawn every frame until it changes.
#[derive(Clone, Debug)]
pub struct Panel {
    pub corner: Corner,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Installed and turned off by the player.
    Off,
    Running,
    /// Could not load or start, or stopped while running.
    Failed(String),
}

/// One installed mod, for the Mods page.
pub struct Entry {
    pub path: PathBuf,
    /// None when the manifest could not be read (`status` says why).
    pub manifest: Option<Manifest>,
    pub status: Status,
    /// The mod's average time per call of its busiest hook.
    pub cost: Duration,
    /// The most fuel one call has used, the hook it was (`tick`, `frame`, `command /name`, ...), and that hook's
    /// fuel limit: how near the mod has come to being stopped for taking too long.
    pub fuel_peak: (u64, &'static str, u64),
    /// The fuel its last call used.
    pub fuel_last: u64,
    run: Option<Running>,
    modified: Option<SystemTime>,
}

struct Running {
    store: Store<State>,
    exports: HorizonMod,
    hooks: Registration,
}

struct State {
    name: String,
    id: String,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    panel: Option<Panel>,
    /// Capability `storage`'s folder; made on first `set`.
    data_dir: PathBuf,
    /// Bytes `storage::set` may still write before this hook call ends
    /// (reset per call in [`Mods::call`]): fuel bounds a mod's CPU time but
    /// not disk I/O time, so a loop of small-fuel, large-payload `set`s
    /// needs its own budget.
    storage_budget: u64,
    limits: StoreLimits,
    /// Capability `input`: claimed item ids, keybinds, queued events.
    claims: Vec<String>,
    keys: Vec<ModKey>,
    events: Vec<InputEvent>,
    /// What was typed on its screen since the last `poll-typed`.
    typed: Vec<Typed>,
    /// Capability `ui`: what the open screen draws (None: closed), and the
    /// cursor while it is open.
    screen: Option<Vec<Draw>>,
    cursor: Option<Point>,
    /// Capability `render`.
    post_source: Option<(Arc<str>, u64)>,
    post_params: Option<[f32; POST_PARAMS]>,
    post_data: Option<(Arc<[u32]>, u64)>,
    /// Capability `world`: answers to its `surface-height` queries, for its next `poll-surface-heights`.
    surface: Vec<SurfaceHeights>,
    /// Capability `world`: answers to its `read-columns` queries, for its next `poll-columns`.
    columns: Vec<ColumnBlocks>,
    /// Capability `world`: the flying speed it wants the player to have.
    flying_speed: Option<f32>,
    /// Feature `fuel-trace` (the soak harness): the call's fuel, and what was left of it as the mod last called the
    /// game; a log line from the mod starting `fuel:` says how much the call had used by then.
    #[cfg(feature = "fuel-trace")]
    fuel_call: u64,
    #[cfg(feature = "fuel-trace")]
    fuel_now: u64,
}

/// The installed mods and the engine running them. `Mods::default()` is
/// the state with none: no engine, and every hook a no-op.
#[derive(Default)]
pub struct Mods {
    engine: Option<(Engine, Linker<State>)>,
    entries: Vec<Entry>,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    /// The cursor over a mod's open screen, from its centre.
    cursor: Option<(f32, f32)>,
}

impl Mods {
    /// Reads every mod at `paths` and starts those `enabled` says are on.
    /// A command that `reserved` (vanilla's commands) or an earlier mod
    /// already uses is refused.
    pub fn load(
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        let mut mods = Mods::default();
        for path in paths {
            let modified = modified(path);
            let mut entry = Entry {
                path: path.clone(),
                manifest: None,
                status: Status::Off,
                cost: Duration::ZERO,
                fuel_peak: (0, "", 0),
                fuel_last: 0,
                run: None,
                modified,
            };
            match Package::read(path) {
                Err(e) => entry.status = Status::Failed(e),
                Ok(package) => {
                    let on = enabled(&package.manifest.id);
                    entry.manifest = Some(package.manifest.clone());
                    if on {
                        // Content is read before the world loads; a mod whose
                        // content is refused stays off, with the reason.
                        let content = crate::Content::read(path, &package.manifest);
                        entry.status =
                            match content.and_then(|_| mods.start(path, &package, &reserved)) {
                                Ok(run) => {
                                    entry.run = run;
                                    Status::Running
                                }
                                Err(e) => Status::Failed(e),
                            };
                    }
                }
            }
            if let (Status::Failed(reason), Some(m)) = (&entry.status, &entry.manifest) {
                log::warn!("Horizon: mod {} did not start: {reason}", m.id);
            }
            mods.entries.push(entry);
        }
        mods
    }

    fn engine(&mut self) -> Result<&(Engine, Linker<State>), String> {
        if self.engine.is_none() {
            let mut config = Config::new();
            config
                .wasm_component_model(true)
                .consume_fuel(true)
                .wasm_backtrace(true);
            let engine = Engine::new(&config).map_err(|e| e.to_string())?;
            let mut linker = Linker::new(&engine);
            HorizonMod::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            self.engine = Some((engine, linker));
        }
        Ok(self.engine.as_ref().expect("made above"))
    }

    /// Starts a mod's code; `Ok(None)` for a content-only mod, which has
    /// nothing to run (the game reads its content before the world loads).
    /// `path` is the mod's own `.hmod` or folder, for capability `storage`'s
    /// per-mod directory.
    fn start(
        &mut self,
        path: &Path,
        package: &Package,
        reserved: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Running>, String> {
        let m = &package.manifest;
        let Some(wasm) = &package.wasm else {
            return Ok(None);
        };
        if m.side == Side::Server {
            return Err("server mods do not run yet".into());
        }
        // Two running mods with the same id would share one `data_dir`
        // (capability `storage`): the first loaded keeps the id.
        if self
            .entries
            .iter()
            .any(|e| e.run.is_some() && e.manifest.as_ref().is_some_and(|other| other.id == m.id))
        {
            return Err(format!("another running mod already uses id {}", m.id));
        }
        let taken: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| r.hooks.commands.iter().cloned())
            .collect();
        let snapshot = self.snapshot.clone();
        let (engine, linker) = self.engine()?;
        let component =
            Component::new(engine, wasm).map_err(|e| format!("not a component: {e}"))?;
        let ty = component.component_type();
        let imports: Vec<String> = ty
            .imports(engine)
            .map(|(name, _)| name.to_string())
            .collect();
        m.check_imports(imports.iter().map(String::as_str))?;
        let data_dir = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(crate::manifest::STORAGE_DIR)
            .join(&m.id);
        let state = State {
            name: m.name.clone(),
            id: m.id.clone(),
            snapshot,
            out: Vec::new(),
            panel: None,
            data_dir,
            storage_budget: STORAGE_WRITE_BUDGET,
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY)
                .instances(16)
                .build(),
            claims: Vec::new(),
            keys: Vec::new(),
            events: Vec::new(),
            typed: Vec::new(),
            screen: None,
            cursor: None,
            post_source: None,
            post_params: None,
            post_data: None,
            surface: Vec::new(),
            columns: Vec::new(),
            flying_speed: None,
            #[cfg(feature = "fuel-trace")]
            fuel_call: FUEL_INIT,
            #[cfg(feature = "fuel-trace")]
            fuel_now: FUEL_INIT,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        #[cfg(feature = "fuel-trace")]
        store.call_hook(|mut ctx, hook| {
            if matches!(hook, wasmtime::CallHook::CallingHost) {
                let f = ctx.get_fuel().unwrap_or(0);
                ctx.data_mut().fuel_now = f;
            }
            Ok(())
        });
        store.set_fuel(FUEL_INIT).map_err(|e| e.to_string())?;
        let exports =
            HorizonMod::instantiate(&mut store, &component, linker).map_err(|e| reason(&e))?;
        let mut hooks = exports
            .horizon_mods_events()
            .call_init(&mut store)
            .map_err(|e| reason(&e))??;
        hooks.commands.retain(|c| {
            let free = !reserved(c) && !taken.contains(c);
            if !free {
                log::warn!("Horizon: mod {} cannot take /{c}: already a command", m.id);
            }
            free
        });
        self.out.append(&mut store.data_mut().out);
        Ok(Some(Running {
            store,
            exports,
            hooks,
        }))
    }

    /// True when at least one mod runs: the game skips its mod work otherwise.
    pub fn is_running(&self) -> bool {
        self.entries.iter().any(|e| e.run.is_some())
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    /// What mods read until the next snapshot.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = Arc::new(snapshot);
    }

    /// Calls `f` on every running mod whose hooks `wants`, with `fuel`.
    fn each(
        &mut self,
        hook: &'static str,
        fuel: u64,
        wants: impl Fn(&Registration) -> bool,
        mut f: impl FnMut(&HorizonMod, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        for i in 0..self.entries.len() {
            if self.entries[i]
                .run
                .as_ref()
                .is_some_and(|r| wants(&r.hooks))
            {
                self.call(i, hook, fuel, &mut f);
            }
        }
    }

    /// One call into mod `i`; a trap or an overrun turns the mod off.
    fn call<R>(
        &mut self,
        i: usize,
        hook: &'static str,
        fuel: u64,
        f: impl FnOnce(&HorizonMod, &mut Store<State>) -> wasmtime::Result<R>,
    ) -> Option<R> {
        let entry = &mut self.entries[i];
        let run = entry.run.as_mut()?;
        run.store.data_mut().snapshot = self.snapshot.clone();
        run.store.data_mut().storage_budget = STORAGE_WRITE_BUDGET;
        let screen_open = run.store.data().screen.is_some();
        run.store.data_mut().cursor = self
            .cursor
            .filter(|_| screen_open)
            .map(|(x, y)| Point { x, y });
        let started = Instant::now();
        #[cfg(feature = "fuel-trace")]
        {
            run.store.data_mut().fuel_call = fuel;
        }
        let result = run
            .store
            .set_fuel(fuel)
            .and_then(|()| f(&run.exports, &mut run.store));
        // An average over about the last 64 calls.
        entry.cost = (entry.cost * 63 + started.elapsed()) / 64;
        // (fuel used: all of it when it ran out)
        let used = fuel.saturating_sub(run.store.get_fuel().unwrap_or(0));
        entry.fuel_last = used;
        if used > entry.fuel_peak.0 {
            entry.fuel_peak = (used, hook, fuel);
        }
        if used >= fuel / FUEL_WARN && result.is_ok() {
            log::warn!(
                "Horizon: mod {}'s {hook} used {:.1}M of its {:.0}M fuel ({:.0}%, {:.1} ms)",
                run.store.data().id,
                used as f64 / 1e6,
                fuel as f64 / 1e6,
                used as f64 * 100.0 / fuel as f64,
                started.elapsed().as_secs_f64() * 1000.0,
            );
        }
        self.out.append(&mut run.store.data_mut().out);
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                let reason = reason(&e);
                log::warn!("Horizon: mod {} stopped: {reason}", run.store.data().id);
                // (where: the hook, and the mod's functions on the stack when it stopped, innermost first)
                log::warn!(
                    "Horizon: mod {} stopped in its {hook} ({:.1}M fuel used, {:.1} ms){}",
                    run.store.data().id,
                    used as f64 / 1e6,
                    started.elapsed().as_secs_f64() * 1000.0,
                    backtrace(&e),
                );
                let name = run.store.data().name.clone();
                self.out.push(Output::Stopped {
                    name,
                    reason: reason.clone(),
                });
                entry.status = Status::Failed(reason);
                entry.run = None;
                None
            }
        }
    }

    /// A game tick.
    pub fn tick(&mut self, tick: u64) {
        self.each(
            "tick",
            FUEL_TICK,
            |h| h.tick,
            |m, s| m.horizon_mods_events().call_tick(s, tick),
        );
    }
    /// A rendered frame, `seconds` after the last.
    pub fn frame(&mut self, seconds: f32) {
        self.each(
            "frame",
            FUEL_FRAME,
            |h| h.frame,
            |m, s| m.horizon_mods_events().call_frame(s, seconds),
        );
    }
    /// A chat line the player received, as plain text.
    pub fn chat(&mut self, text: &str) {
        self.each(
            "chat",
            FUEL_EVENT,
            |h| h.chat,
            |m, s| m.horizon_mods_events().call_chat(s, text),
        );
    }
    /// Whether a running mod owns `/name`.
    pub fn has_command(&self, name: &str) -> bool {
        self.command_owner(name).is_some()
    }
    fn command_owner(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.run
                .as_ref()
                .is_some_and(|r| r.hooks.commands.iter().any(|c| c == name))
        })
    }
    /// Runs `line` (without its slash) if a mod owns its command; false
    /// when none does.
    pub fn command(&mut self, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(name) else {
            return false;
        };
        let result = self.call(i, "command", FUEL_EVENT, |m, s| {
            m.horizon_mods_events().call_command(s, name, args)
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(Output::Feedback { text, error: false }),
            Some(Err(text)) => self.out.push(Output::Feedback { text, error: true }),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The commands running mods own, for suggestions.
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| r.hooks.commands.iter().map(String::as_str))
    }

    /// The running mods' HUD panels.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().panel.as_ref())
    }
    /// What mods asked to show since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    /// The running mod with manifest id `id`.
    fn running(&self, id: &str) -> Option<&Running> {
        self.entries
            .iter()
            .filter(|e| e.manifest.as_ref().is_some_and(|m| m.id == id))
            .find_map(|e| e.run.as_ref())
    }
    fn running_mut(&mut self, id: &str) -> Option<&mut Running> {
        self.entries
            .iter_mut()
            .filter(|e| e.manifest.as_ref().is_some_and(|m| m.id == id))
            .find_map(|e| e.run.as_mut())
    }
    /// The id of the running mod that claims the use of item `item`.
    pub fn claimant(&self, item: &str) -> Option<&str> {
        self.entries.iter().find_map(|e| {
            let run = e.run.as_ref()?;
            run.store
                .data()
                .claims
                .iter()
                .any(|c| c == item)
                .then(|| run.store.data().id.as_str())
        })
    }
    /// The running mods' keybinds.
    pub fn keys(&self) -> Vec<ModKey> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| r.store.data().keys.iter().cloned())
            .collect()
    }
    /// Queues an input event for mod `id`'s next `poll`.
    pub fn push_event(&mut self, id: &str, event: InputEvent) {
        if let Some(run) = self.running_mut(id) {
            let events = &mut run.store.data_mut().events;
            if events.len() >= EVENTS_KEPT {
                events.remove(0);
            }
            events.push(event);
        }
    }
    /// Queues what was typed on mod `id`'s open screen for its next `poll-typed`.
    pub fn push_typed(&mut self, id: &str, typed: Typed) {
        if let Some(run) = self.running_mut(id)
            && run.store.data().screen.is_some()
        {
            let queue = &mut run.store.data_mut().typed;
            if queue.len() >= EVENTS_KEPT {
                queue.remove(0);
            }
            queue.push(typed);
        }
    }
    /// The answer to mod `id`'s `surface-height` query, for its next `poll-surface-heights`.
    pub fn push_surface(&mut self, id: &str, answer: SurfaceHeights) {
        if let Some(run) = self.running_mut(id) {
            let queue = &mut run.store.data_mut().surface;
            if queue.len() >= SURFACE_KEPT {
                queue.remove(0);
            }
            queue.push(answer);
        }
    }
    /// The answer to mod `id`'s `read-columns` query, for its next `poll-columns`.
    pub fn push_columns(&mut self, id: &str, answer: ColumnBlocks) {
        if let Some(run) = self.running_mut(id) {
            let queue = &mut run.store.data_mut().columns;
            if queue.len() >= COLUMNS_KEPT {
                queue.remove(0);
            }
            queue.push(answer);
        }
    }
    /// What mod `id`'s screen draws, while it is open.
    pub fn screen(&self, id: &str) -> Option<&[Draw]> {
        self.running(id)?.store.data().screen.as_deref()
    }
    /// The player closed mod `id`'s screen.
    pub fn screen_closed(&mut self, id: &str) {
        if let Some(run) = self.running_mut(id)
            && run.store.data_mut().screen.take().is_some()
        {
            run.store.data_mut().typed.clear();
            self.push_event(id, InputEvent::ScreenClosed);
        }
    }
    /// The cursor over an open mod screen, from its centre in GUI pixels.
    pub fn set_cursor(&mut self, cursor: Option<(f32, f32)>) {
        self.cursor = cursor;
    }
    /// The first running mod's post-processing pass, when it has a source
    /// and parameters.
    pub fn post(&self) -> Option<Post> {
        self.entries.iter().find_map(|e| {
            let s = e.run.as_ref()?.store.data();
            let (source, version) = s.post_source.clone()?;
            Some(Post {
                source,
                version,
                params: s.post_params?,
                data: s.post_data.clone(),
            })
        })
    }

    /// The flying speed the first running mod that set one wants (capability
    /// `world`); none when no running mod wants one.
    pub fn flying_speed(&self) -> Option<f32> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().flying_speed)
    }

    /// Restarts the mods whose file changed since they loaded (mod
    /// development: rebuild, and the game picks it up). True when any did.
    pub fn reload_changed(&mut self, reserved: impl Fn(&str) -> bool) -> bool {
        let mut any = false;
        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            let now = modified(&entry.path);
            let was_on = entry.status != Status::Off;
            if now == entry.modified || !was_on {
                continue;
            }
            any = true;
            self.entries[i].modified = now;
            self.entries[i].run = None;
            let path = self.entries[i].path.clone();
            let started = Package::read(&path).and_then(|p| {
                self.entries[i].manifest = Some(p.manifest.clone());
                self.start(&path, &p, &reserved)
            });
            let entry = &mut self.entries[i];
            let id = entry.manifest.as_ref().map_or("?", |m| m.id.as_str());
            match started {
                Ok(run) => {
                    log::info!("Horizon: mod {id} reloaded");
                    entry.run = run;
                    entry.status = Status::Running;
                }
                Err(e) => {
                    log::warn!("Horizon: mod {id} did not reload: {e}");
                    entry.status = Status::Failed(e);
                }
            }
        }
        any
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        // A folder mod changes when its component does.
        return std::fs::metadata(path.join(crate::manifest::WASM))
            .ok()?
            .modified()
            .ok();
    }
    meta.modified().ok()
}

/// The mod's functions on the stack when `e` was raised, innermost first (their names, when the mod keeps its name
/// section), as one log line's tail.
fn backtrace(e: &wasmtime::Error) -> String {
    let Some(bt) = e.downcast_ref::<wasmtime::WasmBacktrace>() else {
        return String::new();
    };
    let frames: Vec<String> = bt
        .frames()
        .iter()
        .take(24)
        .map(|f| match f.func_name() {
            Some(n) => n.to_string(),
            None => format!("#{}", f.func_index()),
        })
        .collect();
    format!(": {}", frames.join(" < "))
}

/// A trap or error, as the player reads it.
fn reason(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "it took too long (an endless loop?)".into(),
        Some(Trap::UnreachableCodeReached) => "it crashed (a panic)".into(),
        Some(trap) => format!("it crashed: {trap}"),
        None => format!("{e:#}"),
    }
}

fn clip(text: String) -> String {
    match text.char_indices().nth(LINE_CHARS) {
        Some((at, _)) => text[..at].to_string(),
        None => text,
    }
}

impl mod_log::Host for State {
    fn write(&mut self, level: mod_log::Level, message: String) {
        let level = match level {
            mod_log::Level::Debug => log::Level::Debug,
            mod_log::Level::Info => log::Level::Info,
            mod_log::Level::Warn => log::Level::Warn,
            mod_log::Level::Error => log::Level::Error,
        };
        #[cfg(feature = "fuel-trace")]
        if message.starts_with("fuel:") {
            let used = self.fuel_call.saturating_sub(self.fuel_now);
            log::log!(
                level,
                "[mod {}] {} @ {:.2}M",
                self.id,
                clip(message),
                used as f64 / 1e6
            );
            return;
        }
        log::log!(level, "[mod {}] {}", self.id, clip(message));
    }
}
impl game::Host for State {
    fn get_player(&mut self) -> Option<Player> {
        self.snapshot.player.clone()
    }
    fn get_clock(&mut self) -> Option<Clock> {
        self.snapshot.clock
    }
}
impl chat::Host for State {
    fn show(&mut self, text: String) {
        let shown = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Chat { .. }))
            .count();
        if shown < CHAT_PER_CALL {
            let name = self.name.clone();
            self.out.push(Output::Chat {
                name,
                text: clip(text),
            });
        }
    }
}
impl hud::Host for State {
    fn set_panel(&mut self, corner: Corner, lines: Vec<Line>) {
        let lines = lines
            .into_iter()
            .take(HUD_LINES)
            .map(|l| Line {
                text: clip(l.text),
                color: l.color & 0xFF_FFFF,
            })
            .collect();
        self.panel = Some(Panel { corner, lines });
    }
    fn clear(&mut self) {
        self.panel = None;
    }
}
impl storage::Host for State {
    fn get(&mut self, key: String) -> Option<Vec<u8>> {
        storage_key_ok(&key)
            .then(|| std::fs::read(self.data_dir.join(&key)).ok())
            .flatten()
    }
    fn set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        if !storage_key_ok(&key) {
            return Err(format!("{key:?} is not [a-z0-9_.-]{{1,64}}"));
        }
        let len = value.len() as u64;
        if len > self.storage_budget {
            return Err("too much storage traffic in one call".into());
        }
        // Everything already on disk but the file this call replaces.
        let existing: Vec<std::fs::DirEntry> = std::fs::read_dir(&self.data_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy() != key)
            .collect();
        if existing.len() >= STORAGE_MAX_KEYS {
            return Err(format!("would hold over {STORAGE_MAX_KEYS} keys"));
        }
        let used: u64 = existing
            .iter()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        if used + len > STORAGE_QUOTA {
            return Err(format!(
                "would use over {} MiB of storage",
                STORAGE_QUOTA >> 20
            ));
        }
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join(&key), value).map_err(|e| e.to_string())?;
        self.storage_budget -= len;
        Ok(())
    }
    fn delete(&mut self, key: String) {
        if storage_key_ok(&key) {
            let _ = std::fs::remove_file(self.data_dir.join(&key));
        }
    }
}

impl input::Host for State {
    fn claim_items(&mut self, ids: Vec<String>) {
        self.claims = ids.into_iter().take(CLAIMS).collect();
    }
    fn register_key(&mut self, name: String, label: String, key: String) {
        let key = ModKey {
            mod_id: self.id.clone(),
            mod_name: self.name.clone(),
            name: clip(name),
            label: clip(label),
            key: clip(key),
        };
        if self.keys.contains(&key) {
            return;
        }
        self.keys.retain(|k| k.name != key.name);
        if self.keys.len() < KEYS {
            self.keys.push(key);
            self.out.push(Output::Keys);
        }
    }
    fn held(&mut self) -> Option<HeldItem> {
        self.snapshot.held.clone()
    }
    fn poll(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events)
    }
    fn poll_typed(&mut self) -> Vec<Typed> {
        std::mem::take(&mut self.typed)
    }
}
impl ui::Host for State {
    fn show(&mut self, mut draws: Vec<Draw>) {
        draws.truncate(UI_DRAWS);
        for d in &mut draws {
            if let Draw::Text((text, ..)) = d {
                *text = clip(std::mem::take(text));
            }
        }
        if self.screen.replace(draws).is_none() {
            let id = self.id.clone();
            self.out.push(Output::Screen { id, open: true });
        }
    }
    fn close(&mut self) {
        if self.screen.take().is_some() {
            self.typed.clear();
            let id = self.id.clone();
            self.out.push(Output::Screen { id, open: false });
        }
    }
    fn cursor(&mut self) -> Option<Point> {
        self.cursor
    }
}
impl render::Host for State {
    fn set_post(&mut self, wgsl: String) {
        if wgsl.len() > POST_SOURCE {
            log::warn!(
                "[mod {}] post pass over {} KiB, ignored",
                self.id,
                POST_SOURCE >> 10
            );
            return;
        }
        if self.post_source.as_ref().is_some_and(|(s, _)| **s == *wgsl) {
            return;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.id.hash(&mut h);
        wgsl.hash(&mut h);
        self.post_source = Some((Arc::from(wgsl), h.finish()));
    }
    fn set_params(&mut self, params: Vec<f32>) {
        let mut p = [0.0; POST_PARAMS];
        for (to, from) in p.iter_mut().zip(params) {
            *to = if from.is_finite() { from } else { 0.0 };
        }
        self.post_params = Some(p);
    }
    fn clear_post(&mut self) {
        self.post_source = None;
        self.post_params = None;
        self.post_data = None;
    }
    fn set_data(&mut self, words: Vec<u32>) {
        if words.len() > POST_DATA_WORDS {
            log::warn!(
                "[mod {}] post pass data over {} KiB, ignored",
                self.id,
                (POST_DATA_WORDS * 4) >> 10
            );
            return;
        }
        if self.post_data.as_ref().is_some_and(|(d, _)| **d == *words) {
            return;
        }
        let version = self.post_data.as_ref().map_or(1, |(_, v)| v + 1);
        self.post_data = Some((Arc::from(words), version));
    }
    fn shake(&mut self, strength: f32, seconds: f32) {
        let shakes = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Shake { .. }))
            .count();
        if shakes < SHAKES_PER_CALL && strength.is_finite() && seconds.is_finite() && strength > 0.0
        {
            self.out.push(Output::Shake {
                strength: strength.min(1.0),
                seconds: seconds.clamp(0.05, 30.0),
            });
        }
    }
}
impl world::Host for State {
    fn raycast(&mut self, distance: f32) -> Option<Hit> {
        let max = distance.clamp(0.0, RAYCAST_MAX);
        self.snapshot
            .view_hit
            .as_ref()
            .filter(|(d, _)| *d <= max)
            .map(|(_, hit)| hit.clone())
    }
    fn run_commands(&mut self, commands: Vec<String>) {
        let queued: usize = self
            .out
            .iter()
            .map(|o| match o {
                Output::Commands { commands, .. } => commands.len(),
                _ => 0,
            })
            .sum();
        let room = COMMANDS_PER_CALL.saturating_sub(queued);
        if commands.len() > room {
            log::warn!(
                "[mod {}] over {COMMANDS_PER_CALL} commands in one call; the rest dropped",
                self.id
            );
        }
        let commands: Vec<String> = commands.into_iter().take(room).collect();
        if !commands.is_empty() {
            let id = self.id.clone();
            self.out.push(Output::Commands { id, commands });
        }
    }
    fn edit_blocks(&mut self, edit: BlockEdit) {
        if edit.runs.len() > EDIT_RUNS
            || edit.palette.len() > EDIT_PALETTE
            || edit.filters.len() > EDIT_PALETTE
        {
            log::warn!(
                "[mod {}] edit-blocks over {EDIT_RUNS} runs or {EDIT_PALETTE} palette entries or filters; left out",
                self.id
            );
            return;
        }
        let palette = edit.palette.len();
        let filters = edit.filters.len();
        if let Some(bad) = edit
            .runs
            .iter()
            .find(|r| r.block as usize >= palette || r.only.is_some_and(|f| f as usize >= filters))
        {
            log::warn!(
                "[mod {}] edit-blocks: a run at {} {} {} names palette entry {} or filter {:?}, past the end; left out",
                self.id,
                bad.x,
                bad.y,
                bad.z,
                bad.block,
                bad.only
            );
            return;
        }
        if !edit.runs.is_empty() {
            let id = self.id.clone();
            self.out.push(Output::Edit { id, edit });
        }
    }
    fn surface_height(&mut self, tag: u32, columns: Vec<(i32, i32)>, heightmap: Heightmap) {
        if columns.len() > SURFACE_COLUMNS {
            log::warn!(
                "[mod {}] surface-height over {SURFACE_COLUMNS} columns; left out",
                self.id
            );
            return;
        }
        let id = self.id.clone();
        self.out.push(Output::Surface {
            id,
            tag,
            columns,
            heightmap,
        });
    }
    fn poll_surface_heights(&mut self) -> Vec<SurfaceHeights> {
        std::mem::take(&mut self.surface)
    }
    fn read_columns(
        &mut self,
        tag: u32,
        columns: Vec<(i32, i32)>,
        heightmap: Heightmap,
        below: u32,
        above: u32,
    ) {
        let per = u64::from(below) + u64::from(above);
        if columns.len() > COLUMNS_MAX
            || per > u64::from(COLUMN_BLOCKS)
            || columns.len() as u64 * per > COLUMNS_BLOCKS_MAX as u64
        {
            log::warn!(
                "[mod {}] read-columns over {COLUMNS_MAX} columns, {COLUMN_BLOCKS} blocks a column or \
                 {COLUMNS_BLOCKS_MAX} blocks; left out",
                self.id
            );
            return;
        }
        let id = self.id.clone();
        self.out.push(Output::Columns {
            id,
            tag,
            columns,
            heightmap,
            below,
            above,
        });
    }
    fn poll_columns(&mut self) -> Vec<ColumnBlocks> {
        std::mem::take(&mut self.columns)
    }
    fn set_flying_speed(&mut self, speed: Option<f32>) {
        self.flying_speed = speed
            .filter(|s| s.is_finite())
            .map(|s| s.clamp(0.0, FLYING_SPEED_MAX));
    }
    fn play_sound(&mut self, event: String, at: Option<(f64, f64, f64)>, volume: f32, pitch: f32) {
        let sounds = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Sound { .. }))
            .count();
        if sounds < SOUNDS_PER_CALL {
            self.out.push(Output::Sound {
                event: clip(event),
                at: at.map(|(x, y, z)| [x, y, z]),
                volume: if volume.is_finite() {
                    volume.clamp(0.0, 16.0)
                } else {
                    1.0
                },
                pitch: if pitch.is_finite() {
                    pitch.clamp(0.0, 4.0)
                } else {
                    1.0
                },
            });
        }
    }
}

/// A storage key: one path segment of `[a-z0-9_.-]{1,64}`, not `.` or `..`.
/// No `/`, so a key can never leave the mod's own directory.
fn storage_key_ok(key: &str) -> bool {
    let chars_ok = (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
    chars_ok && key != "." && key != ".."
}
