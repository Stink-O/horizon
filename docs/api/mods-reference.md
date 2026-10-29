<!-- Generated from crates/horizon-mod-sdk/wit/horizon.wit and the manifest's capabilities by crates/horizon-mods/tests/api_reference.rs; a test fails when this file is out of date. Update it with: HORIZON_WRITE_DOCS=1 cargo test -p horizon-mods --features pack --test api_reference -->
# Mod API reference

The contract between Horizon and its mods: the WIT package `horizon:mods@0.1.0`, every interface, type, function and hook in it, and the capabilities a manifest may list. The guide is [mods.md](../mods.md); stability and versions: [stability.md](stability.md). Manifest `api` lines this game serves: `0.1`, `0.2`.

## Capabilities

| In `capabilities` | Grants | The player reads "May …" |
| --- | --- | --- |
| `game-read` | [`game`](#game) | see your position, health and the time |
| `chat` | [`chat`](#chat) | show you chat messages |
| `hud` | [`hud`](#hud) | draw text on your screen |
| `storage` | [`storage`](#storage) | save its own settings to disk |
| `registries` | data files (`assets/`, `data/`), no interface | add blocks and items to your worlds |
| `input` | [`input`](#input) | react to its items, its keys, and clicks and typing on its screen |
| `ui` | [`ui`](#ui) | open a screen of its own |
| `render` | [`render`](#render) | draw effects over the world |
| `world` | [`world`](#world) | change the world you play in as an operator would, set your flying speed and play sounds |

## World `horizon-mod`

- Imports (what the game offers): `log`, `game`, `chat`, `hud`, `storage`, `input`, `ui`, `render`, `world`
- Exports (what the mod provides): `events`

## `log`

Access: always granted.

The mod's log, in the game's log under the mod's id. Always granted.

### enum `level`

| Case | |
| --- | --- |
| `debug` |  |
| `info` |  |
| `warn` |  |
| `error` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `write` | `func(level: level, message: string)` | One line in the game's log as `[mod <id>] message`, at that level. |

## `game`

Access: capability `game-read`.

Capability `game-read`: the local player and the world clock.

### record `player`

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Degrees, as F3 shows them. |
| `pitch` | `f32` | Degrees: -90 looks straight up, 90 straight down. |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |
| `food` | `u32` | 0 to 20, as the hunger bar. |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |

### record `clock`

| Field | Type | |
| --- | --- | --- |
| `game-time` | `u64` | Ticks since the world was created. |
| `day-time` | `u64` | 24000 ticks per day; 0 is 6:00 on the first day. |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-player` | `func() -> option<player>` | None on the title screen. |
| `get-clock` | `func() -> option<clock>` | None on the title screen. |

## `chat`

Access: capability `chat`.

Capability `chat`: messages only the local player sees.

| Function | Signature | What it does |
| --- | --- | --- |
| `show` | `func(text: string)` | Shown in chat after the mod's name, like a system message. |

## `hud`

Access: capability `hud`.

Capability `hud`: one panel of text lines per mod, drawn by the game every frame until the mod changes or clears it (no call per frame).

### enum `corner`

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `line`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Plain text, up to 256 characters. |
| `color` | `u32` | 0xRRGGBB. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-panel` | `func(corner: corner, lines: list<line>)` | Replaces the mod's panel: up to 16 lines in that corner of the screen. |
| `clear` | `func()` | Removes the mod's panel. |

## `storage`

Access: capability `storage`.

Capability `storage`: a per-mod folder of small key/value files, quota'd (docs/mods.md). Keys are `[a-z0-9_.-]{1,64}`, one path segment.

| Function | Signature | What it does |
| --- | --- | --- |
| `get` | `func(key: string) -> option<list<u8>>` | None if `key` has never been `set`, or the key is not well-formed. |
| `set` | `func(key: string, value: list<u8>) -> result<_, string>` | Errs on a malformed key or over quota (bytes for the whole mod). |
| `delete` | `func(key: string)` | No error if `key` was already absent. |

## `input`

Access: capability `input`.

Capability `input`: right-clicks with items the mod claims, keybinds it registers, and clicks and typing on its screen (capability `ui`). Events queue up between calls; the mod collects them with `poll`, usually each tick.

### record `held-item`

An item stack as the mod sees it.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:spyglass`, `mymod:wand`, ... |
| `count` | `u32` |  |
| `slot` | `u32` | 0-8 the hotbar, 40 the offhand. |
| `custom-name` | `option<string>` | `minecraft:custom_name` as plain text. |
| `custom-data` | `option<string>` | `minecraft:custom_data` as SNBT. |

### record `item-use`

| Field | Type | |
| --- | --- | --- |
| `item` | `held-item` |  |
| `ticks` | `u32` | Game ticks the button was held (0 on `use-start`). |

### record `click`

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` | GUI pixels from the centre of the screen (as `ui.show` draws). |
| `y` | `f32` |  |
| `button` | `u32` | 0 left, 1 right, 2 middle. |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `use-start` | `item-use` | The use button went down with a claimed item in hand. The game does nothing else with the click: no zoom, no placing, no swing. |
| `use-stop` | `item-use` | It came up again (or the player switched slots, or a screen opened). |
| `key-down` | `string` | A registered key went down or up while playing (no screen open). |
| `key-up` | `string` |  |
| `screen-click` | `click` | A click on the mod's screen. |
| `screen-closed` |  | The player closed the mod's screen (Escape). |

### variant `typed`

A key pressed on the mod's screen (capability `ui`), held keys repeating. Escape is the game's: it closes the screen.

| Case | Payload | |
| --- | --- | --- |
| `text` | `string` | The characters it typed, as a text box takes them (no control characters). |
| `key` | `string` | A key that types nothing (Backspace, Delete, Enter, Tab, the arrows, Home, End, ...), as options.txt writes keys (`key.keyboard.backspace`, `key.keyboard.enter`). |

| Function | Signature | What it does |
| --- | --- | --- |
| `claim-items` | `func(ids: list<string>)` | Items whose use the mod handles instead of the game, by id. Replaces the previous list. |
| `register-key` | `func(name: string, label: string, key: string)` | A keybind, listed in Options > Controls under the mod's name, where the player can rebind it: `name` is the mod's own (it comes back in `key-down`), `label` what Controls shows, `key` the default as options.txt writes keys (`key.keyboard.r`). |
| `held` | `func() -> option<held-item>` | The stack in the main hand, as of this tick. |
| `poll` | `func() -> list<event>` | The events since the last call, oldest first (at most 256 are kept). |
| `poll-typed` | `func() -> list<typed>` | What was typed on the mod's screen since the last call, oldest first (at most 256 are kept; what is left when the screen closes is dropped). A mod with a text field calls it with `poll`, each tick. |

## `ui`

Access: capability `ui`.

Capability `ui`: one screen the mod owns. The mod hands the game what to draw; the game draws it every frame, releases the mouse, and sends clicks and Escape back as `input` events. It does not pause the game.

### record `rect`

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |
| `w` | `f32` |  |
| `h` | `f32` |  |

### record `point`

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |

### variant `draw`

| Case | Payload | |
| --- | --- | --- |
| `fill` | `tuple<rect, u32>` | A filled rectangle, colour 0xAARRGGBB. |
| `quad` | `tuple<list<point>, u32>` | Any four-cornered shape (wheel segments, arrows), 0xAARRGGBB. |
| `sprite` | `tuple<string, rect, u32>` | A GUI sprite (`gui/sprites/...`, or a mod's `gui/...` texture) stretched over a rectangle, tinted 0xAARRGGBB. |
| `item` | `tuple<string, string, point, f32>` | An item, drawn as the hotbar draws it, at 16 x 16 times `scale`. The second string is an item model id to use instead (or empty). |
| `text` | `tuple<string, point, u32, bool>` | Text with a shadow, 0xRRGGBB; centred on `x` when the bool is set. |

| Function | Signature | What it does |
| --- | --- | --- |
| `show` | `func(draws: list<draw>)` | Opens the mod's screen (or redraws it): coordinates are GUI pixels from the screen's centre, drawn in order. |
| `close` | `func()` | Closes the mod's screen, if it is open (the player's Escape closes it too). |
| `cursor` | `func() -> option<point>` | The cursor in GUI pixels from the centre while the screen is open. |

## `render`

Access: capability `render`.

Capability `render`: a post-processing pass the mod supplies, drawn over the world (under the hand and the HUD), before a shader pack's own post-processing, if any.

| Function | Signature | What it does |
| --- | --- | --- |
| `set-post` | `func(wgsl: string)` | The pass: a WGSL fragment shader with an `fs_main` entry, after the mod pass prelude (docs/mods.md, "A mod's post pass": the world's colour and depth, a `Frame` uniform and helpers). Compiled once per distinct source; an error is logged and the pass skipped. |
| `set-params` | `func(params: list<f32>)` | Sixteen floats the pass reads as `frame.mod_params[0..4]`, kept until changed. The pass runs only while some are set. |
| `clear-post` | `func()` | Stops the pass: no source, no parameters and no data until the next `set-post` and `set-params`. |
| `set-data` | `func(words: list<u32>)` | Words the pass reads as `mod_data` (`var<storage, read> mod_data: array<u32>`, binding 5 of the prelude; `bitcast<f32>` for floats), kept until changed or the pass is cleared. Until some are set it holds one word, 0: read `arrayLength(&mod_data)` before indexing. Uploaded to the GPU only when they change. At most 262,144 words (1 MiB) a call; more are ignored (logged). |
| `shake` | `func(strength: f32, seconds: f32)` | Shakes the camera: a jolt of `strength` (0 to 1, 1 the hardest) that fades out over `seconds` (at most 30). Jolts add up. The game turns the view (not the player) with smooth noise, scaled by the player's Distortion Effects option. |

## `world`

Access: capability `world`.

Capability `world`: the world around the player, and changes to it. Changes need a world this game runs (single player, or hosting LAN); on a remote server they are ignored and logged.

### record `hit`

| Field | Type | |
| --- | --- | --- |
| `block-x` | `s32` | The block hit and the point on its face. |
| `block-y` | `s32` |  |
| `block-z` | `s32` |  |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `block` | `string` | `minecraft:stone`, ... |

### enum `heightmap`

Which of a column's heights a run's `y` counts from (vanilla's heightmaps): one above the highest block that is not air, that blocks motion or holds a fluid, that does so and is not leaves, or that is solid.

| Case | |
| --- | --- |
| `world-surface` |  |
| `motion-blocking` |  |
| `motion-blocking-no-leaves` |  |
| `ocean-floor` |  |

### record `block-run`

A run of blocks up one column: `height` blocks from `y` up, set to palette entry `block`.

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `z` | `s32` |  |
| `y` | `s32` | The lowest block it changes: a height in the world, or with `surface` the blocks over (under, when negative) the column's height on that heightmap. |
| `height` | `u32` |  |
| `surface` | `option<heightmap>` |  |
| `block` | `u32` | An index into the edit's `palette`. |
| `only` | `option<u32>` | An index into the edit's `filters`: only the blocks that match it change. None changes every block of the run. |

### record `block-edit`

Block changes made at once, straight into the world's chunk sections (see `edit-blocks`).

| Field | Type | |
| --- | --- | --- |
| `palette` | `list<string>` | The block states the runs set, as commands write them: `minecraft:stone`, `minecraft:oak_log[axis=x]`. |
| `filters` | `list<string>` | What a run may require of the block it replaces, as `fill ... replace` takes it: `minecraft:air`, `#minecraft:logs`. |
| `runs` | `list<block-run>` | Made in order, each seeing the changes of those before it. |

### record `surface-heights`

The answer to a `surface-height` query: its `tag`, and for each of its columns, in order, the height asked for (one above the highest block that counts on that heightmap, as `edit-blocks` reads it), or none where the column's chunk is not loaded.

| Field | Type | |
| --- | --- | --- |
| `tag` | `u32` |  |
| `heights` | `list<option<s32>>` |  |

### record `column-blocks`

The answer to a `read-columns` query: its `tag`; the names of the blocks it saw (`minecraft:snow`, without their properties), which `blocks` index; for each of its columns, in order, the height it counted from (as `surface-height` answers it), or none where the column's chunk is not loaded; and each column's blocks, `below` + `above` of them a column, bottom first: from `below` under that height up to `above` - 1 over it (a column that is not loaded, and what is outside the world's height, reads as air).

| Field | Type | |
| --- | --- | --- |
| `tag` | `u32` |  |
| `palette` | `list<string>` |  |
| `bases` | `list<option<s32>>` |  |
| `blocks` | `list<u16>` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `raycast` | `func(distance: f32) -> option<hit>` | The first block along the player's view (fluids pass), up to `distance` blocks (at most 1024; past the loaded chunks, across the distant terrain when LOD Distance is on), as of this tick. |
| `run-commands` | `func(commands: list<string>)` | Runs commands as the server would (operator level, no chat feedback), in order, right after this call returns, all in this tick. |
| `play-sound` | `func(event: string, at: option<tuple<f64, f64, f64>>, volume: f32, pitch: f32)` | A sound event (vanilla's or a resource pack's) at a point, or following the player when `at` is none. |
| `set-flying-speed` | `func(speed: option<f32>)` | The local player's flying speed (creative flight), as vanilla's abilities hold it: 0.05 is vanilla's, sprinting doubles it, at most 50. The game keeps it until the mod changes it or stops, through respawns, game mode changes and other worlds; none (or a value that is not a number) puts vanilla's back. A spectator keeps the scroll wheel's speed. On a remote server it is ignored and logged. |
| `edit-blocks` | `func(edit: block-edit)` | Sets many blocks at once, far faster than `fill` and `setblock`: a run per span of a column, each block its own palette entry if need be. Made on the world's thread right after this call returns, in order with `run-commands`, as `fill ... strict` makes them: nothing round them is told (no neighbour or shape updates, no drops, sand and gravel stay up, water and lava keep still), and a block entity goes with its block. Light is worked out again once for each chunk changed, off the world's thread, each chunk is remeshed once, and the players who see a chunk are sent it whole. Heights (`surface`) are read before any of the edit's changes. Only loaded chunks change, within the world's height; the rest of a run is left out. At most 1,048,576 runs and 4,096 palette entries and filters a call. A block or filter that does not parse, or an index past the end, leaves the whole edit out (logged). On a remote server it is ignored and logged. |
| `surface-height` | `func(tag: u32, columns: list<tuple<s32, s32>>, heightmap: heightmap)` | Asks the world for the height of each of `columns` (x, z) on `heightmap`. The world is read where `edit-blocks` changes it, on its own thread, in order with `run-commands` and `edit-blocks` (a query made after an edit sees it), and the answer comes back through `poll-surface-heights`, with the `tag` given here: a tick or two later in single player (the world answers between two of its ticks; the mod never waits on it). Chunks a mod keeps loaded (`forceload`) are read as any other. At most 65,536 columns a call; a query with more is left out (logged). On a remote server every height is none, answered at once. |
| `poll-surface-heights` | `func() -> list<surface-heights>` | The answers to `surface-height` queries that came back since the last call, oldest first. |
| `read-columns` | `func(tag: u32, columns: list<tuple<s32, s32>>, heightmap: heightmap, below: u32, above: u32)` | Asks the world for the blocks of each of `columns` (x, z) round its height on `heightmap`: `below` blocks under it and `above` from it up. Read and answered as `surface-height` is, in order with `run-commands` and `edit-blocks`, through `poll-columns` with the `tag` given here, a tick or two later in single player. At most 65,536 columns, 64 blocks a column and 1,048,576 blocks a call; a query with more is left out (logged). On a remote server every column is none, answered at once. |
| `poll-columns` | `func() -> list<column-blocks>` | The answers to `read-columns` queries that came back since the last call, oldest first. |

## `events`

Access: exported by the mod: the game calls it.

What the game calls. Only the hooks `init` asks for are ever called.

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<string>` | Chat commands, without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `tick` | `bool` | Call `tick` every game tick (20 per second while playing). |
| `frame` | `bool` | Call `frame` every rendered frame. Budget: 0.1 ms. |
| `chat` | `bool` | Call `chat` for each chat line the player receives. |

| Function | Signature | What it does |
| --- | --- | --- |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is shown. |
| `tick` | `func(tick: u64)` | Each game tick while in a world, with the game's tick counter. |
| `frame` | `func(seconds: f32)` | Each rendered frame, with the seconds since the last one. |
| `command` | `func(name: string, args: string) -> result<option<string>, string>` | `/name args`: ok(some(text)) is shown as feedback, err(text) in red. |
| `chat` | `func(text: string)` | Each chat line the player receives, as plain text. |
