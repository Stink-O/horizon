//! Horizon mods in the game (`docs/mods.md`): loading the ones turned on,
//! their tick, frame, chat and command hooks, their HUD panels and what
//! they print. With no mod on, `horizon_mods::Mods` holds no engine and
//! every hook here returns after one check.
use super::{Game, ItemStack, Screen};
use horizon_client::{
    command::{Dispatcher, vanilla},
    extras::Corner,
    extras_hud::{self, Corners},
    hud::{HudBatch, rgba},
    keybinds,
    mod_list::{self, Installed},
    text::{self, Text},
};
use horizon_mods::{
    Click, Clock, Draw, HeldItem, Hit, InputEvent, ItemUse, Mods, Output, Player, Snapshot, Status,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use winit::{event::MouseButton, keyboard::KeyCode};

#[derive(Default)]
pub(super) struct ModsState {
    pub(super) mods: Mods,
    /// `--dev-mod PATH`: always on, reloaded when rebuilt.
    pub(super) dev: Vec<PathBuf>,
    /// When dev mods were last checked for a rebuild.
    checked: Option<Instant>,
    /// The chat's command tree while a running mod owns commands.
    tree: Option<Dispatcher>,
    /// The mod whose screen is open (`Screen::Mod`).
    pub(super) screen: Option<String>,
    /// The mod claiming the item in use (`Game.using`), for its `use-stop`.
    pub(super) using: Option<String>,
}
impl ModsState {
    /// The chat's command tree: vanilla's, plus the running mods' commands.
    pub(super) fn dispatcher(&self) -> &Dispatcher {
        self.tree
            .as_ref()
            .unwrap_or_else(|| vanilla::game_dispatcher())
    }
}
/// How often dev mods are checked for a rebuild.
const RELOAD_CHECK: Duration = Duration::from_secs(1);

/// A vanilla command name: mods may not take it.
fn vanilla_command(name: &str) -> bool {
    let d = vanilla::game_dispatcher();
    d.nodes[0]
        .children
        .iter()
        .any(|&c| d.nodes[c].name() == name)
}

/// `root` with the content mods that are on layered over it
/// (docs/modpacks.md): their blocks, items, data and assets. Read once at
/// start, before the resources load; a mod whose content is refused is left
/// out here and its row on the Mods page says why.
pub(super) fn content_root(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) -> std::path::PathBuf {
    use horizon_client::mod_content::{self, ContentMod};
    use horizon_mods::{Capability, Content, Package};
    let here = std::env::current_dir().unwrap_or_default();
    let mut paths = horizon_mods::find(&here.join(mod_list::DIR));
    paths.retain(|p| !dev.contains(p));
    paths.extend(dev.iter().cloned());
    let mut mods: Vec<ContentMod> = Vec::new();
    for path in &paths {
        let Ok(package) = Package::read(path) else {
            continue;
        };
        let m = &package.manifest;
        let on = enabled.contains(&m.id) || dev.contains(path);
        if !on
            || !m.capabilities.contains(&Capability::Registries)
            || mods.iter().any(|c| c.id == m.id)
        {
            continue;
        }
        let content = Content::read(path, m).and_then(|content| {
            // The first of two mods owning one namespace stays on.
            let own = (m.id.as_str(), content.namespaces.as_slice());
            match mods
                .iter()
                .find_map(|c| horizon_mods::content::clash((&c.id, &c.content.namespaces), own))
            {
                Some(e) => Err(e),
                None => Ok(content),
            }
        });
        match content {
            Ok(content) => mods.push(ContentMod {
                id: m.id.clone(),
                content,
            }),
            Err(e) => log::warn!("Horizon: mod {} content not loaded: {e}", m.id),
        }
    }
    match mod_content::resource_root(root, &mods, &here.join(".horizon-cache")) {
        Ok(out) => {
            if out != root {
                log::info!(
                    "Horizon: content from {} mod(s) in {}",
                    mods.len(),
                    out.display()
                );
            }
            out
        }
        Err(e) => {
            log::warn!("Horizon: content mods not loaded: {e}");
            root.to_owned()
        }
    }
}

impl Game<'_> {
    /// Reads `horizon-mods/` and the dev mods, and starts the ones turned on.
    pub(super) fn start_mods(&mut self) {
        let dir = Self::options_path().with_file_name(mod_list::DIR);
        let dev = &self.mods.dev;
        let mut paths = horizon_mods::find(&dir);
        paths.retain(|p| !dev.contains(p));
        paths.extend(dev.iter().cloned());
        // A dev mod is on whatever the list says.
        let dev_ids: Vec<String> = dev
            .iter()
            .filter_map(|p| horizon_mods::Package::read(p).ok())
            .map(|p| p.manifest.id)
            .collect();
        let enabled = &self.settings.mods.enabled;
        let on = |id: &str| enabled.contains(id) || dev_ids.iter().any(|d| d == id);
        let mods = Mods::load(&paths, on, vanilla_command);
        self.mods.mods = mods;
        self.list_mods();
        self.sync_mod_commands();
        self.show_mod_output();
    }
    /// The Mods page's rows, from what loaded.
    fn list_mods(&mut self) {
        let dev = &self.mods.dev;
        self.settings.mods.installed = self
            .mods
            .mods
            .entries()
            .iter()
            .map(|e| {
                let file = e.path.file_name().unwrap_or_default().to_string_lossy();
                let problem = match &e.status {
                    Status::Failed(reason) => Some(format!("Stopped: {reason}")),
                    _ => None,
                };
                let Some(m) = &e.manifest else {
                    return Installed {
                        id: String::new(),
                        title: file.into_owned(),
                        description: String::new(),
                        problem,
                        dev: false,
                    };
                };
                // What it may do first: the row cuts long descriptions.
                let may: Vec<&str> = m.capabilities.iter().map(|c| c.describe()).collect();
                let may = if may.is_empty() {
                    "Asks for nothing.".to_owned()
                } else {
                    format!("May {}.", may.join(", "))
                };
                Installed {
                    id: m.id.clone(),
                    title: format!("{} {}", m.name, m.version),
                    description: format!("{may} {}", m.description).trim().to_owned(),
                    problem,
                    dev: dev.contains(&e.path),
                }
            })
            .collect();
    }
    /// The chat's tree after the running mods changed.
    fn sync_mod_commands(&mut self) {
        let mut names = self.mods.mods.commands().peekable();
        self.mods.tree = names
            .peek()
            .is_some()
            .then(|| vanilla::game_dispatcher().with_mod_commands(names));
    }
    /// After the options changed: restart the mods if the list of the ones
    /// turned on did.
    pub(super) fn sync_mods(&mut self) {
        let running: Vec<&str> = self
            .mods
            .mods
            .entries()
            .iter()
            .filter(|e| e.status != Status::Off)
            .filter_map(|e| Some(e.manifest.as_ref()?.id.as_str()))
            .collect();
        let wanted: Vec<&str> = self
            .settings
            .mods
            .installed
            .iter()
            .enumerate()
            .filter(|&(i, _)| self.settings.mods.is_on(i))
            .map(|(_, m)| m.id.as_str())
            .collect();
        if running != wanted {
            self.start_mods();
        }
    }
    /// What mods read this tick.
    /// Past the resident chunks: the view walked across the distant terrain's ground heights (`lod::Lod::ground`),
    /// a block at a time, so a mod can aim as far as Horizon draws. Columns in loaded chunks are left to `clip_ray`,
    /// which has already missed them.
    fn lod_ray(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
    ) -> Option<([i32; 3], [f64; 3], horizon_world::registry::BlockStateId)> {
        let lod = self.lod.as_ref()?;
        let mut t = 0.0;
        while t <= reach {
            let p = [
                eye[0] + dir[0] * t,
                eye[1] + dir[1] * t,
                eye[2] + dir[2] * t,
            ];
            let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
            if !self.world.loaded(x, z)
                && let Some((height, state)) = lod.ground(x, z)
                && p[1] <= f64::from(height)
            {
                return Some(([x, height - 1, z], [p[0], f64::from(height), p[2]], state));
            }
            t += 1.0;
        }
        None
    }
    fn mod_snapshot(&self) -> Snapshot {
        let player = self.player.as_ref().map(|p| Player {
            x: p.position.x,
            y: p.position.y,
            z: p.position.z,
            yaw: self.yaw,
            pitch: self.pitch,
            health: p.health.health(),
            food: p.health.food.food.max(0) as u32,
            dimension: self.dim.id().to_owned(),
        });
        let clock = player.is_some().then(|| Clock {
            game_time: self.world.game_time.max(0) as u64,
            day_time: self.world.day_time.max(0) as u64,
        });
        let held = player.as_ref().and(self.mod_held(self.selected));
        let view_hit = player.as_ref().and_then(|_| {
            let eye = self.eye_position()?;
            let dir = super::view_vector(self.yaw, self.pitch);
            let reach = f64::from(horizon_mods::RAYCAST_MAX);
            let (pos, point, state) = match self.clip_ray(eye, dir, reach) {
                Some(hit) => (
                    hit.pos,
                    hit.point,
                    self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]),
                ),
                None => self.lod_ray(eye, dir, reach)?,
            };
            let distance = (0..3)
                .map(|i| (point[i] - eye[i]).powi(2))
                .sum::<f64>()
                .sqrt();
            Some((
                distance as f32,
                Hit {
                    block_x: pos[0],
                    block_y: pos[1],
                    block_z: pos[2],
                    x: point[0],
                    y: point[1],
                    z: point[2],
                    block: self.res.facts().get(state).name.to_string(),
                },
            ))
        });
        Snapshot {
            player,
            clock,
            held,
            view_hit,
        }
    }
    /// The stack in inventory slot `slot` as mods see it.
    fn mod_held(&self, slot: usize) -> Option<HeldItem> {
        let stack = self.stack_in(slot)?;
        let extra = stack.extra.as_deref();
        Some(HeldItem {
            id: stack.item.to_string(),
            count: stack.count,
            slot: slot as u32,
            custom_name: extra.and_then(|e| e.custom_name.clone()),
            custom_data: extra
                .and_then(|e| e.other("minecraft:custom_data"))
                .map(horizon_client::snbt::to_snbt),
        })
    }
    /// A game tick.
    pub(super) fn tick_mods(&mut self) {
        if !self.mods.mods.is_running() {
            return;
        }
        let snapshot = self.mod_snapshot();
        self.mods.mods.set_snapshot(snapshot);
        let ticks = self.ticks;
        horizon_client::spans::time("tick_mods", || self.mods.mods.tick(ticks));
        self.show_mod_output();
    }
    /// Once per frame: the frame hook, and dev mods' rebuilds.
    pub(super) fn mods_frame(&mut self, elapsed: Duration) {
        if !self.mods.dev.is_empty()
            && self
                .mods
                .checked
                .is_none_or(|t| t.elapsed() >= RELOAD_CHECK)
        {
            self.mods.checked = Some(Instant::now());
            if self.mods.mods.reload_changed(vanilla_command) {
                self.list_mods();
                self.sync_mod_commands();
                self.system_message(Text::literal("Mods reloaded").with_color(text::GRAY));
            }
        }
        if !self.mods.mods.is_running() {
            return;
        }
        let cursor = (self.screen == Screen::Mod).then(|| {
            let size = self
                .renderer
                .as_ref()
                .map_or([1, 1], |r| [r.config.width, r.config.height]);
            let b = HudBatch::new(size);
            (
                self.cursor.0 - b.width / 2.0,
                self.cursor.1 - b.height / 2.0,
            )
        });
        self.mods.mods.set_cursor(cursor);
        self.mods.mods.frame(elapsed.as_secs_f32());
        self.show_mod_output();
    }
    /// `/command` typed in chat: true when a mod owns it.
    pub(super) fn mod_command(&mut self, command: &str) -> bool {
        if !self.mods.mods.is_running() {
            return false;
        }
        // The command may follow a teleport in the same tick.
        let snapshot = self.mod_snapshot();
        self.mods.mods.set_snapshot(snapshot);
        if !self.mods.mods.command(command) {
            return false;
        }
        self.show_mod_output();
        true
    }
    /// A chat line the player received.
    pub(super) fn mods_chat(&mut self, line: &str) {
        self.mods.mods.chat(line);
        self.show_mod_output();
    }
    /// Chat lines, command feedback and stopped mods, into the chat; the
    /// screens, commands and sounds mods asked for.
    fn show_mod_output(&mut self) {
        for output in self.mods.mods.drain() {
            let line = match output {
                Output::Screen { id, open: true } => {
                    self.open_mod_screen(id);
                    continue;
                }
                Output::Screen { id, open: false } => {
                    if self.mods.screen.as_deref() == Some(id.as_str()) {
                        self.mods.screen = None;
                        if self.screen == Screen::Mod {
                            self.screen = Screen::Playing;
                            self.capture(true);
                        }
                    }
                    continue;
                }
                Output::Keys => {
                    self.sync_mod_keys();
                    continue;
                }
                Output::Commands { id, commands } => {
                    self.run_mod_commands(&id, commands);
                    continue;
                }
                Output::Sound {
                    event,
                    at,
                    volume,
                    pitch,
                } => {
                    self.audio.play(&event, at, volume, pitch);
                    continue;
                }
                Output::Shake { strength, seconds } => {
                    let now = self.ticks as f64 / 20.0;
                    self.camera_shake.add(now, strength, seconds);
                    continue;
                }
                Output::Chat { name, text } => {
                    let mut line = Text::literal(format!("[{name}] ")).with_color(text::GRAY);
                    line.push(Text::literal(text));
                    line
                }
                Output::Feedback { text, error: false } => Text::literal(text),
                Output::Feedback { text, error: true } => Text::literal(text).with_color(text::RED),
                Output::Stopped { name, reason } => {
                    self.list_mods();
                    self.sync_mod_commands();
                    Text::literal(format!("Mod {name} stopped: {reason}")).with_color(text::RED)
                }
            };
            // Not through system_message: a mod must not hear its own lines.
            let focused = self.screen == Screen::Chat;
            self.ui
                .chat
                .add(&self.gui.atlas, line, None, self.ticks, focused);
        }
    }
    /// The mods' HUD panels, stacked with the extras in their corners.
    pub(super) fn draw_mod_panels(&self, batch: &mut HudBatch, corners: &mut Corners) {
        for panel in self.mods.mods.panels() {
            let corner = match panel.corner {
                horizon_mods::Corner::TopLeft => Corner::TopLeft,
                horizon_mods::Corner::TopRight => Corner::TopRight,
                horizon_mods::Corner::BottomLeft => Corner::BottomLeft,
                horizon_mods::Corner::BottomRight => Corner::BottomRight,
            };
            // F3 takes the top left, as it does from the extras.
            if self.debug && corner == Corner::TopLeft {
                continue;
            }
            let lines: Vec<(&str, u32)> = panel
                .lines
                .iter()
                .map(|l| (l.text.as_str(), l.color))
                .collect();
            extras_hud::draw_lines(batch, &self.gui.atlas, corners, corner, &lines);
        }
    }
}

impl Game<'_> {
    // ---- capability `input` ------------------------------------------------

    /// `use_hand`: a right-click with an item a mod claims starts its use for
    /// that mod and nothing else happens. True when a mod took it.
    pub(super) fn mod_use_start(&mut self) -> bool {
        if !self.mods.mods.is_running() || self.guest_use.is_some() || self.using.is_some() {
            return false;
        }
        let slot = self.held_slot();
        let Some(item) = self.mod_held(slot) else {
            return false;
        };
        let Some(id) = self.mods.mods.claimant(&item.id).map(str::to_owned) else {
            return false;
        };
        self.using = Some((slot, 0));
        self.mods
            .mods
            .push_event(&id, InputEvent::UseStart(ItemUse { item, ticks: 0 }));
        self.mods.using = Some(id);
        true
    }
    /// `tick_used_item` for a claimed item: counts the ticks held and ends
    /// the use when the button comes up. True when the item in use is one.
    pub(super) fn mod_use_tick(&mut self, slot: usize, ticks: u32, released: bool) -> bool {
        let Some(id) = self.mods.using.clone() else {
            return false;
        };
        let still_claimed = self
            .stack_in(slot)
            .is_some_and(|s| self.mods.mods.claimant(&s.item) == Some(id.as_str()));
        if released || !still_claimed {
            if let Some(item) = self.mod_held(slot) {
                self.mods
                    .mods
                    .push_event(&id, InputEvent::UseStop(ItemUse { item, ticks }));
            }
            self.mods.using = None;
            self.using = None;
        } else {
            self.using = Some((slot, ticks + 1));
        }
        true
    }
    /// The running mods' keybinds into Controls (`key.<mod id>.<name>`).
    fn sync_mod_keys(&mut self) {
        for k in self.mods.mods.keys() {
            let name = format!("key.{}.{}", k.mod_id, k.name);
            self.settings
                .keys
                .register_mod_key(&name, &k.label, &k.mod_name, &k.key);
        }
    }
    /// A key went down or up: the mods' binds on it (as the player bound
    /// them in Controls), while playing.
    pub(super) fn mods_key(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        if repeat
            || !self.mods.mods.is_running()
            || self.screen != Screen::Playing
            || self.title.is_some()
        {
            return;
        }
        let Some(pressed_name) = keybinds::key_name(key) else {
            return;
        };
        for k in self.mods.mods.keys() {
            let name = format!("key.{}.{}", k.mod_id, k.name);
            if self.settings.keys.get(&name) != pressed_name {
                continue;
            }
            let event = if pressed {
                InputEvent::KeyDown(k.name)
            } else {
                InputEvent::KeyUp(k.name)
            };
            self.mods.mods.push_event(&k.mod_id, event);
        }
    }

    // ---- capability `ui` ---------------------------------------------------

    fn open_mod_screen(&mut self, id: String) {
        if self.screen != Screen::Playing && self.screen != Screen::Mod {
            // Another screen is up: the mod's opens later, if it asks again.
            self.mods.mods.screen_closed(&id);
            return;
        }
        if self.screen == Screen::Mod
            && let Some(other) = self.mods.screen.replace(id.clone())
            && other != id
        {
            self.mods.mods.screen_closed(&other);
        }
        self.mods.screen = Some(id);
        if self.screen != Screen::Mod {
            self.screen = Screen::Mod;
            self.keys.clear();
            self.left = false;
            self.right = false;
            self.mining = None;
            self.capture(false);
        }
    }
    /// Escape on a mod's screen.
    pub(super) fn close_mod_screen(&mut self) {
        if let Some(id) = self.mods.screen.take() {
            self.mods.mods.screen_closed(&id);
        }
        self.screen = Screen::Playing;
        self.capture(true);
    }
    pub(super) fn mod_screen_click(&mut self, button: MouseButton) {
        let Some(id) = self.mods.screen.clone() else {
            return;
        };
        let size = self
            .renderer
            .as_ref()
            .map_or([1, 1], |r| [r.config.width, r.config.height]);
        let b = HudBatch::new(size);
        let button = match button {
            MouseButton::Left => 0,
            MouseButton::Right => 1,
            MouseButton::Middle => 2,
            _ => return,
        };
        let click = Click {
            x: self.cursor.0 - b.width / 2.0,
            y: self.cursor.1 - b.height / 2.0,
            button,
        };
        self.mods
            .mods
            .push_event(&id, InputEvent::ScreenClick(click));
    }
    pub(super) fn draw_mod_screen(&self, batch: &mut HudBatch) {
        let Some(draws) = self
            .mods
            .screen
            .as_deref()
            .and_then(|id| self.mods.mods.screen(id))
        else {
            return;
        };
        let (cx, cy) = (batch.width / 2.0, batch.height / 2.0);
        let argb = |c: u32| {
            let [a, r, g, b] = c.to_be_bytes();
            rgba(r, g, b, a)
        };
        let atlas = &self.gui.atlas;
        for draw in draws {
            match draw {
                Draw::Fill((r, color)) => {
                    batch.fill(atlas, cx + r.x, cy + r.y, r.w, r.h, argb(*color))
                }
                Draw::Quad((points, color)) if points.len() >= 4 => {
                    let p = std::array::from_fn(|i| [cx + points[i].x, cy + points[i].y]);
                    batch.quad(p, [atlas.white_uv(); 4], argb(*color));
                }
                Draw::Quad(_) => {}
                Draw::Sprite((name, r, color)) => {
                    batch.sprite_scaled(atlas, name, cx + r.x, cy + r.y, r.w, r.h, argb(*color));
                }
                Draw::Item((item, model, at, scale)) => {
                    let mut stack = ItemStack::new(item, 1);
                    if !model.is_empty() {
                        let mut data = horizon_client::item_data::StackData::default();
                        data.set_other(
                            "minecraft:item_model",
                            Some(horizon_world::nbt::Tag::String(model.as_str().into())),
                        );
                        stack.extra = Some(Box::new(data));
                    }
                    let start = batch.vertices.len();
                    let (x, y) = (cx + at.x, cy + at.y);
                    self.draw_item_model(batch, &stack, x, y);
                    if (*scale - 1.0).abs() > 1e-3 {
                        batch.scale_since(start, x, y, *scale, *scale);
                    }
                }
                Draw::Text((text, at, color, centred)) => {
                    let [_, r, g, b] = color.to_be_bytes();
                    let x = if *centred {
                        cx + at.x - (HudBatch::text_width(atlas, text) / 2.0).floor()
                    } else {
                        cx + at.x
                    };
                    batch.text(atlas, text, x, cy + at.y, rgba(r, g, b, 255), true);
                }
            }
        }
    }

    // ---- capability `world` ------------------------------------------------

    /// `world.run-commands`: at operator level, silently, in order.
    fn run_mod_commands(&mut self, id: &str, commands: Vec<String>) {
        if self.remote.is_some() {
            log::warn!("[mod {id}] run-commands ignored: the world is on a remote server");
            return;
        }
        if let Some(server) = &self.integrated {
            server.call(move |g| g.run_commands_silently(&commands));
            return;
        }
        self.run_commands_silently(&commands);
    }
    fn run_commands_silently(&mut self, commands: &[String]) {
        // Its own command budget: a mod's batch is not the tick's functions.
        let saved = std::mem::take(&mut self.exec);
        self.as_console(|g| {
            for line in commands {
                let source = super::commands::execute::Source {
                    silent: true,
                    ..g.player_source()
                };
                g.exec = Default::default();
                let result = horizon_client::spans::time(command_span(line), || {
                    g.execute_line(line.trim_start_matches('/'), source)
                });
                if let Err(e) = result {
                    log::debug!("mod command {line:?} failed: {e:?}");
                }
            }
        });
        self.exec = saved;
    }
}

/// The span a mod's command is timed under (`HORIZON_SPIKES`): what it runs, through any `execute ... run`.
fn command_span(line: &str) -> &'static str {
    let run = line
        .rsplit(" run ")
        .next()
        .unwrap_or(line)
        .trim_start_matches('/');
    match run.split_whitespace().next().unwrap_or("") {
        "fill" => "mod_fill",
        "setblock" => "mod_setblock",
        "summon" => "mod_summon",
        "particle" => "mod_particle",
        "playsound" => "mod_playsound",
        "kill" => "mod_kill",
        _ => "mod_other",
    }
}
