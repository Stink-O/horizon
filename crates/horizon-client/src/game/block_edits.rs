//! A mod's `world.edit-blocks` (capability `world`): its palette and filters
//! read as commands read blocks, its heights read, and its runs made at once
//! through `World::edit_runs`, with what `fill ... strict` does for the block
//! entities and the watched blocks of each block changed.
#![cfg(feature = "mods")]
use super::Game;
use horizon_client::{
    block_edit::Run,
    chunk::MIN_Y,
    command::{Reader, args::BlockInput, block},
    containers::BlockEntity,
    fast_hash::FxHashMap,
};
use horizon_mods::{BlockEdit, Heightmap};
use horizon_world::{
    heightmap::HeightmapType, registry::BlockStateId, worldgen::terrain::TerrainBlockStates,
};

impl Game<'_> {
    /// Make a mod's block edit; how many blocks changed, or why none did.
    pub(super) fn edit_blocks(&mut self, edit: &BlockEdit) -> Result<u64, String> {
        let parse = |name: &str, testing: bool| -> Result<BlockInput, String> {
            let mut reader = Reader::new(name);
            let b = block::parse(&mut reader, &self.ui.registries, testing)
                .map_err(|e| format!("{name:?}: {}", e.message.plain()))?;
            if reader.can_read() {
                return Err(format!("{name:?}: more after the block"));
            }
            Ok(b)
        };
        let mut palette = Vec::with_capacity(edit.palette.len());
        for name in &edit.palette {
            let b = parse(name, false)?;
            let state = self
                .block_state(&b)
                .map_err(|e| format!("{name:?}: {}", e.message.plain()))?;
            palette.push(state.0);
        }
        let filters = edit
            .filters
            .iter()
            .map(|f| parse(f, true))
            .collect::<Result<Vec<_>, _>>()?;
        // the heights the runs count from, read before any change, once a column
        let mut heights: FxHashMap<(i32, i32, u8), i32> = FxHashMap::default();
        let mut runs = Vec::with_capacity(edit.runs.len());
        for r in &edit.runs {
            if r.height == 0 {
                continue;
            }
            let base = match r.surface {
                None => i64::from(r.y),
                Some(h) => {
                    let top = *heights
                        .entry((r.x, r.z, h as u8))
                        .or_insert_with(|| self.edit_height(h, r.x, r.z));
                    i64::from(top) + i64::from(r.y)
                }
            };
            let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            runs.push(Run {
                x: r.x,
                z: r.z,
                y0: clamp(base),
                y1: clamp(base + i64::from(r.height) - 1),
                state: palette[r.block as usize],
                only: r.only.map(|f| f as u16),
            });
        }
        // (what the registry, the tags and the facts tell of a state, beside the world being changed)
        let registry = self.pack.assets.registry;
        let tags = &self.res.items().block_tags;
        let facts = self.res.facts();
        let watched = std::mem::take(&mut self.world.watched);
        let out = self.world.edit_runs(
            &runs,
            |f, s| {
                super::commands::block_input_matches(
                    registry,
                    tags,
                    &filters[f as usize],
                    BlockStateId(s),
                )
            },
            |s| {
                watched.get(s as usize).copied().unwrap_or(false)
                    || BlockEntity::for_block(&facts.get(BlockStateId(s)).name).is_some()
            },
        );
        self.world.watched = watched;
        for &((x, y, z), old, new) in &out.tracked {
            let watches = |id: u32| {
                self.world
                    .watched
                    .get(id as usize)
                    .copied()
                    .unwrap_or(false)
            };
            if watches(old) || watches(new) {
                self.world.watched_writes.push(((x, y, z), old, new));
            }
            self.world.block_entities.state_changed((x, y, z), old, new);
            // A different block replaces the block entity, as `place_state` does.
            let (was, now) = (
                self.res.facts().get(BlockStateId(old)).name.clone(),
                self.res.facts().get(BlockStateId(new)).name.clone(),
            );
            if was != now {
                self.jukebox_removed([x, y, z]);
                self.world.block_entities.remove(&(x, y, z));
                if let Some(entity) = BlockEntity::for_block(&now) {
                    self.world.block_entities.insert((x, y, z), entity);
                }
            }
        }
        Ok(out.changed)
    }

    /// `world.surface-height`: each column's height on `h` as `edit_height` reads it, none where its chunk is not
    /// loaded. (A chunk read is looked up once for a run of columns in it.)
    pub(super) fn surface_heights(&self, columns: &[(i32, i32)], h: Heightmap) -> Vec<Option<i32>> {
        let mut last: Option<((i32, i32), bool)> = None;
        columns
            .iter()
            .map(|&(x, z)| {
                let key = (x >> 4, z >> 4);
                let loaded = match last {
                    Some((k, l)) if k == key => l,
                    _ => {
                        let l = self.world.chunks.get(&key).is_some();
                        last = Some((key, l));
                        l
                    }
                };
                loaded.then(|| self.edit_height(h, x, z))
            })
            .collect()
    }

    /// `world.read-columns`: each column's height on `h` (none where its chunk is not loaded) and its blocks from `below`
    /// under it to `above` - 1 over it, bottom first (air outside the world's height and where it is not loaded), the
    /// blocks as indices into the names seen (the tag is the caller's).
    pub(super) fn read_columns(
        &self,
        columns: &[(i32, i32)],
        h: Heightmap,
        below: u32,
        above: u32,
    ) -> horizon_mods::ColumnBlocks {
        let bases = self.surface_heights(columns, h);
        let per = (below + above) as usize;
        let mut blocks = Vec::with_capacity(columns.len() * per);
        let mut palette: Vec<String> = Vec::new();
        // (a state's index into the names, worked out once a state)
        let mut seen: FxHashMap<u32, u16> = FxHashMap::default();
        let facts = self.res.facts();
        let mut index = |s: BlockStateId| -> u16 {
            *seen.entry(s.0).or_insert_with(|| {
                let name = &facts.get(s).name;
                let i = palette
                    .iter()
                    .position(|n| **n == **name)
                    .unwrap_or_else(|| {
                        palette.push(name.to_string());
                        palette.len() - 1
                    });
                i as u16
            })
        };
        let top = MIN_Y + horizon_client::chunk::HEIGHT - 1;
        for (&(x, z), base) in columns.iter().zip(&bases) {
            match base {
                Some(b) => {
                    let y0 = b - below as i32;
                    for y in y0..y0 + per as i32 {
                        let s = if (MIN_Y..=top).contains(&y) {
                            self.world.block(x, y, z)
                        } else {
                            self.world.air
                        };
                        blocks.push(index(s));
                    }
                }
                None => {
                    let air = index(self.world.air);
                    blocks.extend(std::iter::repeat_n(air, per));
                }
            }
        }
        horizon_mods::ColumnBlocks {
            tag: 0,
            palette,
            bases,
            blocks,
        }
    }

    /// One above the highest block of column (x, z) that counts on heightmap `h`, or the bottom of the world (also
    /// where it is not loaded).
    fn edit_height(&self, h: Heightmap, x: i32, z: i32) -> i32 {
        if h == Heightmap::WorldSurface {
            return self
                .world
                .world_surface(x, z, |s| self.res.facts().is_air(s));
        }
        let properties = &self.pack.assets.properties;
        let counts = |t: HeightmapType, s: BlockStateId| t.is_opaque(properties.heightmap_state(s));
        // (as `rain_height` reads it: both others count only blocks that block motion, so they are at or under it)
        let top = self
            .world
            .motion_blocking(x, z, |s| counts(HeightmapType::MotionBlocking, s));
        let kind = match h {
            Heightmap::MotionBlockingNoLeaves => HeightmapType::MotionBlockingNoLeaves,
            Heightmap::OceanFloor => HeightmapType::OceanFloor,
            _ => return top,
        };
        let mut y = top;
        while y > MIN_Y && !counts(kind, self.world.block(x, y - 1, z)) {
            y -= 1;
        }
        y
    }
}
