//! Plays a real mod (its `.hmod` or folder) headless against a small block world, and measures the fuel each of its
//! calls uses: for mods whose strikes or jobs are heavy, a check that no tick, frame or command comes near the limit
//! that stops them ("it took too long").
//!
//!     cargo run --release -p horizon-mods --features runtime --example mod_soak -- MOD_DIR [SEQUENCES] [SEED]
//!
//! Written for Orbital Laser (its commands, its designator), as the sequences a player makes: a laser fired, often
//! at the same spot, stopped part way through and fired again at once, with the frames now quick and now slow and the
//! world's answers to its reads arriving at once, late, or many together. The world is a block store the mod's edits
//! and fills change, so the later strikes land on the craters (and the aquifers and lava they open) of the earlier.
//! As in the game, only the land round the player and the chunks forceloaded a while (as they are generated) is loaded:
//! a read of any other column answers nothing for it. Now and then the next strike is at a fresh spot far away, with
//! the player near it or some hundreds of blocks off, so a strike's reads come back mostly unloaded at first.
//! Prints each hook's worst call; exits 1 if the mod stopped, or any call used over `SOAK_MAX` (percent, default 50)
//! of its fuel.
//! Copy the mod somewhere first: its `storage` is kept beside it.
use horizon_mods::{
    BlockEdit, Clock, ColumnBlocks, Heightmap, HeldItem, Hit, InputEvent, ItemUse, Mods, Output,
    Player, Snapshot, Status, SurfaceHeights,
};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

const MIN_Y: i32 = -64;
const H: usize = 256; // -64 .. 192
const AIR: u8 = 0;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Columns of blocks, made as they are first read or changed.
struct World {
    names: Vec<String>,
    ids: HashMap<String, u8>,
    cols: HashMap<(i32, i32), Box<[u8; H]>>,
    /// The chunks forceloaded, and the tick each is loaded from (generated a while after it is asked for).
    forced: HashMap<(i32, i32), u64>,
    /// The player's chunk and how many chunks round it are loaded; the tick now.
    view: ((i32, i32), i32),
    now: u64,
    /// How slowly forceloaded chunks arrive (times the usual lag; 0: never, as land in the far terrain's LOD may not).
    slow: u64,
}
fn hash(x: i32, z: i32, k: u32) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (z as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(k).wrapping_mul(0x1656_67B1_9E37_79F9);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 11) as f64 / (1u64 << 53) as f64
}
impl World {
    fn new() -> Self {
        let mut w = World {
            names: Vec::new(),
            ids: HashMap::new(),
            cols: HashMap::new(),
            forced: HashMap::new(),
            view: ((0, 0), 10),
            now: 0,
            slow: 1,
        };
        w.id("minecraft:air");
        w
    }
    fn id(&mut self, name: &str) -> u8 {
        let base = name.split('[').next().unwrap_or(name);
        if let Some(&i) = self.ids.get(base) {
            return i;
        }
        let i = self.names.len().min(255) as u8;
        if self.names.len() < 256 {
            self.names.push(base.to_string());
            self.ids.insert(base.to_string(), i);
        }
        i
    }
    fn col(&mut self, x: i32, z: i32) -> &mut [u8; H] {
        if !self.cols.contains_key(&(x, z)) {
            let c = self.generate(x, z);
            self.cols.insert((x, z), c);
        }
        self.cols.get_mut(&(x, z)).expect("made")
    }
    /// Hills round 64 with a sea at 62, deepslate under 0, aquifers (water pockets) at 20-34 and lava at -54..-50,
    /// trees on the grass.
    fn generate(&mut self, x: i32, z: i32) -> Box<[u8; H]> {
        let (stone, deep, dirt, grass, water, lava, bedrock, log, leaves, sand, gravel) = (
            self.id("minecraft:stone"),
            self.id("minecraft:deepslate"),
            self.id("minecraft:dirt"),
            self.id("minecraft:grass_block"),
            self.id("minecraft:water"),
            self.id("minecraft:lava"),
            self.id("minecraft:bedrock"),
            self.id("minecraft:oak_log"),
            self.id("minecraft:oak_leaves"),
            self.id("minecraft:sand"),
            self.id("minecraft:gravel"),
        );
        let (fx, fz) = (f64::from(x), f64::from(z));
        let h = 66.0
            + 8.0 * (fx / 37.0).sin() * (fz / 53.0).cos()
            + 5.0 * (fx / 13.0 + fz / 17.0).sin();
        let top = h as i32;
        let mut c = Box::new([AIR; H]);
        for y in MIN_Y..top.min(MIN_Y + H as i32) {
            let i = (y - MIN_Y) as usize;
            c[i] = if y == MIN_Y {
                bedrock
            } else if y < 0 {
                deep
            } else if y < top - 4 {
                stone
            } else if y < top - 1 {
                dirt
            } else if top <= 63 {
                sand
            } else {
                grass
            };
        }
        // aquifers and lava pockets in blobs
        let blob = |k: u32, s: f64| {
            ((fx / s).sin() * (fz / s * 1.3).cos() + hash(x >> 3, z >> 3, k) * 0.6) > 0.9
        };
        if blob(1, 23.0) {
            for y in 20..34 {
                c[(y - MIN_Y) as usize] = water;
            }
            c[(19 - MIN_Y) as usize] = gravel;
        }
        if blob(2, 31.0) {
            for y in -54..-50 {
                c[(y - MIN_Y) as usize] = lava;
            }
        }
        for y in top..=62 {
            c[(y - MIN_Y) as usize] = water;
        }
        if top > 63 && hash(x, z, 3) < 0.015 {
            for y in top..top + 5 {
                c[(y - MIN_Y) as usize] = log;
            }
            for y in top + 5..top + 7 {
                c[(y - MIN_Y) as usize] = leaves;
            }
        }
        // a tree's crown round its trunk, roughly
        if top > 63 && c[(top - MIN_Y) as usize] == AIR && hash(x >> 1, z >> 1, 4) < 0.05 {
            for y in top + 3..top + 6 {
                c[(y - MIN_Y) as usize] = leaves;
            }
        }
        c
    }
    /// Whether column (x, z) is loaded: round the player, or forceloaded long enough ago to be generated.
    fn loaded(&self, x: i32, z: i32) -> bool {
        let (cx, cz) = (x >> 4, z >> 4);
        let ((px, pz), v) = self.view;
        (cx - px).abs() <= v && (cz - pz).abs() <= v
            || self.forced.get(&(cx, cz)).is_some_and(|&t| t <= self.now)
    }
    fn is(&self, b: u8, what: &str) -> bool {
        self.names.get(b as usize).is_some_and(|n| n.contains(what))
    }
    /// One above the highest block that counts on `h`.
    fn height(&mut self, x: i32, z: i32, h: Heightmap) -> i32 {
        let c = *self.col(x, z);
        for i in (0..H).rev() {
            let b = c[i];
            if b == AIR {
                continue;
            }
            let leaves = self.is(b, "leaves");
            let fluid = self.is(b, "water") || self.is(b, "lava");
            let plant = self.is(b, "grass") && !self.is(b, "grass_block") || self.is(b, "fern");
            let counts = match h {
                Heightmap::WorldSurface => true,
                Heightmap::MotionBlocking => !plant,
                Heightmap::MotionBlockingNoLeaves => !plant && !leaves,
                Heightmap::OceanFloor => !plant && !fluid,
            };
            if counts {
                return MIN_Y + i as i32 + 1;
            }
        }
        MIN_Y
    }
    fn set(&mut self, x: i32, y: i32, z: i32, b: u8, only: Option<&str>) {
        if !(MIN_Y..MIN_Y + H as i32).contains(&y) {
            return;
        }
        let i = (y - MIN_Y) as usize;
        let cur = self.col(x, z)[i];
        if let Some(f) = only {
            let name = self.names[cur as usize].as_str();
            let ok = if let Some(tag) = f.strip_prefix("#minecraft:") {
                let stem = tag.trim_end_matches('s');
                name.contains(stem) || (tag == "replaceable" && name == "minecraft:air")
            } else {
                name == f.split('[').next().unwrap_or(f)
            };
            if !ok {
                return;
            }
        }
        self.col(x, z)[i] = b;
    }
    fn edit(&mut self, e: &BlockEdit) {
        let pal: Vec<u8> = e.palette.iter().map(|n| self.id(n)).collect();
        for r in &e.runs {
            let y0 = match r.surface {
                Some(h) => self.height(r.x, r.z, h) + r.y,
                None => r.y,
            };
            let only = r.only.map(|f| e.filters[f as usize].clone());
            let b = pal[r.block as usize];
            for y in y0..y0.saturating_add(r.height.min(H as u32 + 64) as i32) {
                self.set(r.x, y, r.z, b, only.as_deref());
            }
        }
    }
    /// `fill x0 y0 z0 x1 y1 z1 block [replace filter]` (relative coordinates and other commands left alone).
    fn command(&mut self, c: &str) -> bool {
        let w: Vec<&str> = c.split_whitespace().collect();
        if w.first() == Some(&"forceload") && w.len() >= 4 {
            let n: Vec<i32> = w[2..].iter().filter_map(|s| s.parse().ok()).collect();
            let (x0, z0) = (n[0], n[1]);
            let (x1, z1) = if n.len() >= 4 { (n[2], n[3]) } else { (x0, z0) };
            for cx in (x0.min(x1) >> 4)..=(x0.max(x1) >> 4) {
                for cz in (z0.min(z1) >> 4)..=(z0.max(z1) >> 4) {
                    if w[1] == "add" {
                        // (generated some ticks to some seconds after: the nearer the middle of the batch, the sooner)
                        let lag = (10 + (hash(cx, cz, 9) * 140.0) as u64)
                            .checked_mul(self.slow)
                            .filter(|_| self.slow > 0)
                            .unwrap_or(u64::MAX - self.now);
                        self.forced.entry((cx, cz)).or_insert(self.now + lag);
                    } else if w[1] == "remove" {
                        self.forced.remove(&(cx, cz));
                    }
                }
            }
            return true;
        }
        if w.first() != Some(&"fill") || w.len() < 8 {
            return false;
        }
        let n: Option<Vec<i32>> = w[1..7].iter().map(|s| s.parse().ok()).collect();
        let Some(n) = n else { return false };
        let b = self.id(w[7]);
        let only = (w.get(8) == Some(&"replace"))
            .then(|| w.get(9).map(|s| s.to_string()))
            .flatten();
        let (x0, x1) = (n[0].min(n[3]), n[0].max(n[3]));
        let (y0, y1) = (n[1].min(n[4]), n[1].max(n[4]));
        let (z0, z1) = (n[2].min(n[5]), n[2].max(n[5]));
        if (x1 - x0 + 1) as i64 * (z1 - z0 + 1) as i64 > 1 << 20 {
            return false;
        }
        for x in x0..=x1 {
            for z in z0..=z1 {
                for y in y0..=y1 {
                    self.set(x, y, z, b, only.as_deref());
                }
            }
        }
        true
    }
    fn read(
        &mut self,
        columns: &[(i32, i32)],
        h: Heightmap,
        below: u32,
        above: u32,
    ) -> ColumnBlocks {
        let per = (below + above) as usize;
        let mut palette: Vec<String> = Vec::new();
        let mut map: HashMap<u8, u16> = HashMap::new();
        let mut bases = Vec::with_capacity(columns.len());
        let mut blocks = Vec::with_capacity(columns.len() * per);
        for &(x, z) in columns {
            if !self.loaded(x, z) {
                bases.push(None);
                blocks.extend(std::iter::repeat_n(0, per));
                continue;
            }
            let base = self.height(x, z, h);
            bases.push(Some(base));
            let c = *self.col(x, z);
            for y in base - below as i32..base - below as i32 + per as i32 {
                let b = if (MIN_Y..MIN_Y + H as i32).contains(&y) {
                    c[(y - MIN_Y) as usize]
                } else {
                    AIR
                };
                let n = map.len() as u16;
                let i = *map.entry(b).or_insert_with(|| {
                    palette.push(self.names[b as usize].clone());
                    n
                });
                blocks.push(i);
            }
        }
        let tints = columns
            .iter()
            .flat_map(|_| [0x91BD59, 0x77AB2F, 0x3F76E4])
            .collect();
        ColumnBlocks {
            tag: 0,
            palette,
            bases,
            blocks,
            tints,
        }
    }
}

/// How the world answers reads in a sequence.
#[derive(Clone, Copy, Debug)]
enum Answers {
    /// On the next tick.
    Next,
    /// After 1 to n ticks each.
    Late(u64),
    /// Held, then all at once every n ticks.
    Bunched(u64),
}

enum Ask {
    Surface(u32, Vec<(i32, i32)>, Heightmap),
    Columns(u32, Vec<(i32, i32)>, Heightmap, u32, u32),
}

struct Soak {
    mods: Mods,
    world: World,
    rng: Rng,
    tick: u64,
    answers: Answers,
    pending: VecDeque<(u64, Ask)>,
    /// Seconds each frame takes now.
    frame_s: f32,
    frames: u32,
    target: (i32, i32),
    /// The player's place from the target (blocks east, south).
    me: (i32, i32),
    /// Per hook: the most fuel a call used, and the limit.
    worst: HashMap<&'static str, (u64, u64)>,
    stopped: Option<String>,
    strikes: u32,
    stops: u32,
    /// The spot the strikes are round (moved far off for a fresh one).
    home: (i32, i32),
    /// Edits made, runs in them, reads answered, commands run.
    counts: [u64; 4],
    /// The tick the last strike fired on, reads answered this tick, and the percentage of a hook's fuel over which a
    /// call is printed.
    fired: u64,
    answered: u32,
    report: u64,
    /// The percentage of its fuel over which a call fails the soak, and the calls that went over it.
    max_pct: f64,
    over: Vec<String>,
    log: Vec<String>,
}

const DESIGNATOR: &str = "orbital:laser_designator";

impl Soak {
    fn snapshot(&mut self) -> Snapshot {
        let (tx, tz) = self.target;
        let ty = self.world.height(tx, tz, Heightmap::MotionBlockingNoLeaves);
        let b = self.world.col(tx, tz)[(ty - 1 - MIN_Y).max(0) as usize];
        let block = self.world.names[b as usize].clone();
        Snapshot {
            player: Some(Player {
                x: f64::from(tx + self.me.0) + 0.5,
                y: f64::from(ty) + 40.0,
                z: f64::from(tz + self.me.1) + 0.5,
                yaw: 180.0,
                pitch: 15.0,
                health: 20.0,
                food: 20,
                dimension: "minecraft:overworld".into(),
            }),
            clock: Some(Clock {
                game_time: 1000 + self.tick,
                day_time: 6000,
            }),
            held: Some(HeldItem {
                id: DESIGNATOR.into(),
                count: 1,
                slot: 0,
                custom_name: None,
                custom_data: None,
            }),
            view_hit: Some((
                (self.me.0 as f32).hypot(self.me.1 as f32),
                Hit {
                    block_x: tx,
                    block_y: ty - 1,
                    block_z: tz,
                    x: f64::from(tx) + 0.5,
                    y: f64::from(ty),
                    z: f64::from(tz) + 0.5,
                    block,
                },
            )),
        }
    }
    fn after(&mut self, hook: &'static str, limit: u64) {
        let e = &self.mods.entries()[0];
        let used = e.fuel_last;
        let before = self.counts;
        let heavy = used * 100 > limit * self.report;
        if used as f64 * 100.0 > limit as f64 * self.max_pct {
            self.over.push(format!(
                "{hook} {:.1}M ({:.0}%) {:.2} s after the fire ({})",
                used as f64 / 1e6,
                used as f64 * 100.0 / limit as f64,
                (self.tick.saturating_sub(self.fired)) as f64 * 0.05,
                self.log.join("; ")
            ));
        }
        let w = self.worst.entry(hook).or_insert((0, limit));
        w.0 = w.0.max(used);
        if e.status != Status::Running && self.stopped.is_none() {
            self.stopped = Some(format!(
                "{:?} in {hook} on tick {} ({})",
                e.status,
                self.tick,
                self.log.join("; ")
            ));
        }
        for o in self.mods.drain() {
            match o {
                Output::Edit { edit, .. } => {
                    self.counts[0] += 1;
                    self.counts[1] += edit.runs.len() as u64;
                    self.world.edit(&edit)
                }
                Output::Commands { commands, .. } => {
                    self.counts[3] += commands.len() as u64;
                    for c in &commands {
                        self.world.command(c);
                    }
                }
                Output::Surface {
                    tag,
                    columns,
                    heightmap,
                    ..
                } => {
                    let due = self.due();
                    self.pending
                        .push_back((due, Ask::Surface(tag, columns, heightmap)));
                }
                Output::Columns {
                    tag,
                    columns,
                    heightmap,
                    below,
                    above,
                    ..
                } => {
                    let due = self.due();
                    self.pending
                        .push_back((due, Ask::Columns(tag, columns, heightmap, below, above)));
                }
                Output::Stopped { reason, .. } => {
                    self.stopped.get_or_insert(format!(
                        "{reason} in {hook} on tick {} ({})",
                        self.tick,
                        self.log.join("; ")
                    ));
                }
                _ => {}
            }
        }
        if heavy {
            let d: Vec<u64> = self.counts.iter().zip(before).map(|(a, b)| a - b).collect();
            eprintln!(
                "  heavy {hook}: {:.1}M, {:.2} s after the fire, {} edits ({} runs), {} cmds; {} reads answered before it",
                used as f64 / 1e6,
                (self.tick.saturating_sub(self.fired)) as f64 * 0.05,
                d[0],
                d[1],
                d[3],
                self.answered,
            );
        }
    }
    fn due(&mut self) -> u64 {
        match self.answers {
            Answers::Next => self.tick + 1,
            Answers::Late(n) => self.tick + 1 + self.rng.below(n),
            Answers::Bunched(n) => (self.tick / n + 1) * n,
        }
    }
    fn answer(&mut self) {
        self.answered = 0;
        let mut keep = VecDeque::new();
        while let Some((due, ask)) = self.pending.pop_front() {
            if due > self.tick {
                keep.push_back((due, ask));
                continue;
            }
            self.counts[2] += 1;
            self.answered += 1;
            match ask {
                Ask::Surface(tag, columns, h) => {
                    let heights = columns
                        .iter()
                        .map(|&(x, z)| self.world.loaded(x, z).then(|| self.world.height(x, z, h)))
                        .collect();
                    self.mods
                        .push_surface("orbital", SurfaceHeights { tag, heights });
                }
                Ask::Columns(tag, columns, h, below, above) => {
                    let mut a = self.world.read(&columns, h, below, above);
                    a.tag = tag;
                    self.mods.push_columns("orbital", a);
                }
            }
        }
        self.pending = keep;
    }
    fn step(&mut self) {
        self.tick += 1;
        self.world.now = self.tick;
        let (tx, tz) = self.target;
        self.world.view.0 = ((tx + self.me.0) >> 4, (tz + self.me.1) >> 4);
        self.answer();
        let snap = self.snapshot();
        self.mods.set_snapshot(snap);
        self.mods.tick(self.tick);
        self.after("tick", 50_000_000);
        for _ in 0..self.frames {
            self.mods.frame(self.frame_s);
            self.after("frame", 10_000_000);
        }
    }
    fn command(&mut self, c: &str) {
        self.mods.command(c);
        self.after("command", 100_000_000);
    }
    fn fire(&mut self) {
        let item = HeldItem {
            id: DESIGNATOR.into(),
            count: 1,
            slot: 0,
            custom_name: None,
            custom_data: None,
        };
        self.mods.push_event(
            "orbital",
            InputEvent::UseStart(ItemUse {
                item: item.clone(),
                ticks: 0,
            }),
        );
        self.fired = self.tick + 15;
        for _ in 0..16 {
            self.step();
        }
        self.mods
            .push_event("orbital", InputEvent::UseStop(ItemUse { item, ticks: 16 }));
        self.strikes += 1;
    }
    fn run(&mut self, ticks: u32) {
        for _ in 0..ticks {
            if self.stopped.is_some() {
                return;
            }
            // (the frames: quick, or slow for a while, as when edits pile up)
            if self.rng.below(60) == 0 {
                self.frame_s = [0.007, 0.016, 0.016, 0.045, 0.09, 0.2][self.rng.below(6) as usize];
                self.frames = match self.frame_s {
                    s if s < 0.01 => 7,
                    s if s < 0.02 => 3,
                    s if s < 0.05 => 1,
                    _ => u32::from(self.rng.below(2) == 0),
                };
            }
            self.step();
        }
    }
}

/// The game's log, as the game prints it (the mod's lines and the runtime's warnings).
struct Print;
impl log::Log for Print {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            eprintln!("  [{}] {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

fn main() {
    if std::env::var_os("SOAK_LOG").is_some() {
        log::set_logger(&Print).expect("one logger");
        log::set_max_level(log::LevelFilter::Info);
    }
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().expect("the mod's folder or .hmod"));
    let sequences: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let lasers: Vec<String> = std::env::var("SOAK_LASERS")
        .unwrap_or_else(|_| "nuke".into())
        .split(',')
        .map(String::from)
        .collect();
    let max_pct: f64 = std::env::var("SOAK_MAX")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50.0);
    let mods = Mods::load(&[path], |_| true, |_| false);
    assert_eq!(
        mods.entries()[0].status,
        Status::Running,
        "the mod did not start"
    );
    let mut s = Soak {
        mods,
        world: World::new(),
        rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
        tick: 0,
        answers: Answers::Next,
        pending: VecDeque::new(),
        frame_s: 0.016,
        frames: 3,
        target: (497_062, 498_935),
        me: (0, 150),
        worst: HashMap::new(),
        stopped: None,
        strikes: 0,
        stops: 0,
        home: (497_062, 498_935),
        counts: [0; 4],
        fired: 0,
        answered: 0,
        report: std::env::var("SOAK_REPORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(40),
        max_pct,
        over: Vec::new(),
        log: Vec::new(),
    };
    let terrain = std::env::var("SOAK_TERRAIN").unwrap_or_else(|_| "on".into());
    for c in [
        &format!("orbital terrain {terrain}") as &str,
        "orbital messages on",
        "orbital quality fast",
        "orbital nukecloud 10",
    ] {
        s.command(c);
    }
    if let Ok(m) = std::env::var("SOAK_METER") {
        s.command(&format!("orbital meter {m}"));
    }
    if std::env::var_os("SOAK_TRACE").is_some() {
        s.command("orbital meter trace");
    }
    for _ in 0..40 {
        s.step();
    }
    let started = std::time::Instant::now();
    for n in 0..sequences {
        if s.stopped.is_some() {
            break;
        }
        s.log.clear();
        let laser = lasers[s.rng.below(lasers.len() as u64) as usize].clone();
        s.command(&format!("orbital {laser}"));
        // (at and round the same spot, now and then further off; or at a fresh spot far away, never loaded, the
        // player flown there or some hundreds of blocks off it, the land round them all that is loaded)
        let far = s.rng.below(3) == 0;
        if far {
            let a = s.rng.below(360) as f64 * std::f64::consts::PI / 180.0;
            let d = 2000.0 + s.rng.below(6000) as f64;
            s.home = (
                s.home.0 + (a.cos() * d) as i32,
                s.home.1 + (a.sin() * d) as i32,
            );
            s.world.view.1 = [6, 8, 10, 12][s.rng.below(4) as usize];
        }
        // (the forceloaded chunks generated as usual, slowly, or never while the strike lasts)
        s.world.slow = [1, 1, 4, 20, 0][s.rng.below(5) as usize];
        let (bx, bz) = s.home;
        let off = if far { 0 } else if s.rng.below(4) == 0 { 120 } else { 24 };
        s.target = (
            bx + s.rng.below(2 * off + 1) as i32 - off as i32,
            bz + s.rng.below(2 * off + 1) as i32 - off as i32,
        );
        // (called in from close by, from some hundreds of blocks, or from far off near the designator's 1000-block reach)
        s.me = match s.rng.below(4) {
            0 => (0, 150),
            1 => (0, 380 + s.rng.below(200) as i32),
            2 => (s.rng.below(400) as i32 - 200, 240 + s.rng.below(300) as i32),
            _ => {
                let a = s.rng.below(360) as f64 * std::f64::consts::PI / 180.0;
                let d = 500.0 + s.rng.below(500) as f64;
                ((a.cos() * d) as i32, (a.sin() * d) as i32)
            }
        };
        // (the land round them loaded before the fire: the player is there a few ticks first)
        for _ in 0..4 {
            s.step();
        }
        s.answers = match s.rng.below(4) {
            0 => Answers::Next,
            1 => Answers::Late(10),
            2 => Answers::Late(60),
            _ => Answers::Bunched(10 + s.rng.below(60)),
        };
        s.fire();
        // stopped soon, late, or not at all (and then fired again at once)
        let full = std::env::var_os("SOAK_FULL").is_some();
        // (and, often, early: in the land read's first seconds, or round the burst at 11 s and its first seconds)
        let ticks = match if full { 6 } else { s.rng.below(7) } {
            0 => 1 + s.rng.below(20) as u32,
            1 => 20 + s.rng.below(200) as u32,
            2 => 200 + s.rng.below(600) as u32,
            3 => 30 + s.rng.below(60) as u32,
            4 => 210 + s.rng.below(100) as u32,
            5 => 2 + s.rng.below(60) as u32,
            _ => 2200,
        };
        s.log.push(format!(
            "#{n} {laser} at {:?} from {:?} answers {:?} forceload x{}, {ticks} ticks",
            s.target, s.me, s.answers, s.world.slow
        ));
        s.run(ticks);
        if ticks < 2200 {
            s.command("orbital stop");
            s.stops += 1;
        }
        let gap = [0, 0, 1, 5, 40][s.rng.below(5) as usize];
        s.run(gap);
        eprintln!(
            "{n:4}: {:>10} after {ticks:4} ticks, worst tick {:5.1}M frame {:4.1}M command {:5.1}M  edits {} runs {} reads {} cmds {} cols {} ({:.0} s)",
            s.log
                .last()
                .map(|l| l.split(' ').nth(1).unwrap_or(""))
                .unwrap_or(""),
            s.worst.get("tick").map_or(0.0, |w| w.0 as f64 / 1e6),
            s.worst.get("frame").map_or(0.0, |w| w.0 as f64 / 1e6),
            s.worst.get("command").map_or(0.0, |w| w.0 as f64 / 1e6),
            s.counts[0],
            s.counts[1],
            s.counts[2],
            s.counts[3],
            s.world.cols.len(),
            started.elapsed().as_secs_f64(),
        );
    }
    println!(
        "{} strikes, {} stopped part way, {} ticks",
        s.strikes, s.stops, s.tick
    );
    let mut bad = s.stopped.is_some() || !s.over.is_empty();
    for o in s.over.iter().take(20) {
        println!("OVER {max_pct:.0}%: {o}");
    }
    if s.over.len() > 20 {
        println!("... and {} more calls over {max_pct:.0}%", s.over.len() - 20);
    }
    for (hook, (used, limit)) in &s.worst {
        let pct = *used as f64 * 100.0 / *limit as f64;
        println!(
            "worst {hook}: {:.2}M of {:.0}M fuel ({pct:.1}%)",
            *used as f64 / 1e6,
            *limit as f64 / 1e6
        );
        bad |= pct > max_pct;
    }
    if let Some(why) = &s.stopped {
        println!("STOPPED: {why}");
    }
    std::process::exit(i32::from(bad));
}
