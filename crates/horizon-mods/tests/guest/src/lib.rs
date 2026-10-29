//! A mod for the runtime's tests: `/spin` loops forever, `/panic` panics,
//! `/grow` asks for a gigabyte, `/hello` chats, `/save` and `/load` exercise
//! capability `storage` on a fixed key, `/save_key <key> <text>` exercises
//! it on an arbitrary key (for the quota and key-count limits), `/fly
//! <speed>|none` sets the flying speed, `/screen` opens an empty screen and
//! `/typed` chats what was typed on it, `/surface <tag>` asks for the heights
//! of columns (1, 2) and (-3, 40), `/heights` chats the answers come back,
//! `/data <n>` gives a post pass the words 0..n, `/columns <tag> <n> <below>
//! <above>` asks for the blocks of n columns, `/blocks` chats the answers; `tick` and `frame` do
//! nothing, to measure the cost of a call.
use horizon_mod_sdk::{Mod, Registration, chat, input, render, storage, ui, world};

struct Guest {
    ticks: u64,
}

impl Mod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = [
            "spin", "panic", "grow", "hello", "save", "load", "save_key", "fly", "screen", "typed",
            "surface", "heights", "data", "columns", "blocks",
        ]
        .into_iter()
        .fold(Registration::new(), Registration::command)
        .on_tick()
        .on_frame();
        Ok((Guest { ticks: 0 }, hooks))
    }
    fn tick(&mut self, _tick: u64) {
        self.ticks += 1;
    }
    fn command(&mut self, name: &str, args: &str) -> Result<Option<String>, String> {
        match name {
            "spin" => {
                let mut n = 0u64;
                while std::hint::black_box(n) != u64::MAX {
                    n = n.wrapping_add(2);
                }
                Ok(None)
            }
            "panic" => panic!("asked to"),
            "grow" => {
                let big = std::hint::black_box(vec![1u8; 1 << 30]);
                Ok(Some(format!("{}", big.len())))
            }
            "save" => {
                storage::set("note", args.as_bytes())?;
                Ok(Some("saved".into()))
            }
            "load" => {
                let text =
                    storage::get("note").map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                chat::show(text.as_deref().unwrap_or("(empty)"));
                Ok(None)
            }
            "save_key" => {
                let (key, text) = args
                    .split_once(' ')
                    .ok_or("Usage: /save_key <key> <text>")?;
                storage::set(key, text.as_bytes())?;
                Ok(Some("saved".into()))
            }
            "fly" => {
                world::set_flying_speed(args.parse().ok());
                Ok(None)
            }
            "screen" => {
                ui::show(&[]);
                Ok(None)
            }
            "typed" => {
                let typed: Vec<String> = input::poll_typed()
                    .into_iter()
                    .map(|t| match t {
                        input::Typed::Text(s) => format!("text {s}"),
                        input::Typed::Key(k) => format!("key {k}"),
                    })
                    .collect();
                chat::show(&typed.join(", "));
                Ok(None)
            }
            "surface" => {
                let tag = args.parse().map_err(|_| "Usage: /surface <tag>")?;
                world::surface_height(tag, &[(1, 2), (-3, 40)], world::Heightmap::MotionBlockingNoLeaves);
                Ok(None)
            }
            "heights" => {
                let answers: Vec<String> = world::poll_surface_heights()
                    .into_iter()
                    .map(|a| {
                        let hs: Vec<String> = a
                            .heights
                            .iter()
                            .map(|h| h.map_or("none".into(), |y| y.to_string()))
                            .collect();
                        format!("{}: {}", a.tag, hs.join(" "))
                    })
                    .collect();
                chat::show(&answers.join(", "));
                Ok(None)
            }
            "columns" => {
                let n: Vec<u32> = args.split(' ').filter_map(|w| w.parse().ok()).collect();
                let [tag, count, below, above] = n[..] else {
                    return Err("Usage: /columns <tag> <n> <below> <above>".into());
                };
                let cols: Vec<(i32, i32)> = (0..count as i32).map(|i| (i, -i)).collect();
                world::read_columns(tag, &cols, world::Heightmap::MotionBlockingNoLeaves, below, above);
                Ok(None)
            }
            "blocks" => {
                let answers: Vec<String> = world::poll_columns()
                    .into_iter()
                    .map(|a| {
                        let bases: Vec<String> =
                            a.bases.iter().map(|h| h.map_or("none".into(), |y| y.to_string())).collect();
                        let blocks: Vec<&str> = a.blocks.iter().map(|&b| a.palette[b as usize].as_str()).collect();
                        let tints: Vec<String> = a.tints.iter().map(|t| format!("{t:06x}")).collect();
                        format!("{}: {} | {} | {}", a.tag, bases.join(" "), blocks.join(" "), tints.join(" "))
                    })
                    .collect();
                chat::show(&answers.join(", "));
                Ok(None)
            }
            "data" => {
                let n: u32 = args.parse().map_err(|_| "Usage: /data <n>")?;
                render::set_post("@fragment fn fs_main(in: StageIn) -> @location(0) vec4<f32> { return vec4<f32>(1.0); }");
                render::set_params(&[0.0; 16]);
                render::set_data(&(0..n).collect::<Vec<u32>>());
                Ok(None)
            }
            _ => {
                chat::show(&format!("hello {args} after {} ticks", self.ticks));
                Ok(None)
            }
        }
    }
}

horizon_mod_sdk::export_mod!(Guest);
