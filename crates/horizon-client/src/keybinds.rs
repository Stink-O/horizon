//! Key mappings (Options.keyMappings): vanilla names, categories and
//! defaults, `key_<name>:key.keyboard.x` persistence, display names, and the
//! translation of rebound physical keys back to the default keys the game
//! logic listens for.
use crate::text::Text;
use std::collections::BTreeMap;
use winit::{event::MouseButton, keyboard::KeyCode};

/// (name, default key, category) in Options.keyMappings order.
pub const MAPPINGS: &[(&str, &str, &str)] = &[
    ("key.attack", "key.mouse.left", "gameplay"),
    ("key.use", "key.mouse.right", "gameplay"),
    ("key.forward", "key.keyboard.w", "movement"),
    ("key.left", "key.keyboard.a", "movement"),
    ("key.back", "key.keyboard.s", "movement"),
    ("key.right", "key.keyboard.d", "movement"),
    ("key.jump", "key.keyboard.space", "movement"),
    ("key.sneak", "key.keyboard.left.shift", "movement"),
    ("key.sprint", "key.keyboard.left.control", "movement"),
    ("key.drop", "key.keyboard.q", "inventory"),
    ("key.inventory", "key.keyboard.e", "inventory"),
    ("key.chat", "key.keyboard.t", "multiplayer"),
    ("key.playerlist", "key.keyboard.tab", "multiplayer"),
    ("key.pickItem", "key.mouse.middle", "gameplay"),
    ("key.command", "key.keyboard.slash", "multiplayer"),
    ("key.friends", "key.keyboard.o", "multiplayer"),
    ("key.socialInteractions", "key.keyboard.p", "multiplayer"),
    ("key.toggleGui", "key.keyboard.f1", "misc"),
    (
        "key.toggleSpectatorShaderEffects",
        "key.keyboard.f4",
        "misc",
    ),
    ("key.screenshot", "key.keyboard.f2", "misc"),
    ("key.togglePerspective", "key.keyboard.f5", "misc"),
    ("key.smoothCamera", "key.keyboard.unknown", "misc"),
    ("key.fullscreen", "key.keyboard.f11", "misc"),
    ("key.spectatorOutlines", "key.keyboard.unknown", "spectator"),
    ("key.spectatorHotbar", "key.mouse.middle", "spectator"),
    ("key.swapOffhand", "key.keyboard.f", "inventory"),
    ("key.saveToolbarActivator", "key.keyboard.c", "creative"),
    ("key.loadToolbarActivator", "key.keyboard.x", "creative"),
    ("key.advancements", "key.keyboard.l", "misc"),
    ("key.quickActions", "key.keyboard.g", "misc"),
    ("key.debug.overlay", "key.keyboard.f3", "debug"),
    ("key.debug.modifier", "key.keyboard.f3", "debug"),
    ("key.hotbar.1", "key.keyboard.1", "inventory"),
    ("key.hotbar.2", "key.keyboard.2", "inventory"),
    ("key.hotbar.3", "key.keyboard.3", "inventory"),
    ("key.hotbar.4", "key.keyboard.4", "inventory"),
    ("key.hotbar.5", "key.keyboard.5", "inventory"),
    ("key.hotbar.6", "key.keyboard.6", "inventory"),
    ("key.hotbar.7", "key.keyboard.7", "inventory"),
    ("key.hotbar.8", "key.keyboard.8", "inventory"),
    ("key.hotbar.9", "key.keyboard.9", "inventory"),
    ("key.debug.reloadChunk", "key.keyboard.a", "debug"),
    ("key.debug.showHitboxes", "key.keyboard.b", "debug"),
    ("key.debug.clearChat", "key.keyboard.d", "debug"),
    ("key.debug.crash", "key.keyboard.c", "debug"),
    ("key.debug.showChunkBorders", "key.keyboard.g", "debug"),
    ("key.debug.showAdvancedTooltips", "key.keyboard.h", "debug"),
    ("key.debug.copyRecreateCommand", "key.keyboard.i", "debug"),
    ("key.debug.spectate", "key.keyboard.n", "debug"),
    ("key.debug.switchGameMode", "key.keyboard.f4", "debug"),
    ("key.debug.debugOptions", "key.keyboard.f6", "debug"),
    ("key.debug.focusPause", "key.keyboard.p", "debug"),
    ("key.debug.dumpDynamicTextures", "key.keyboard.s", "debug"),
    ("key.debug.reloadResourcePacks", "key.keyboard.t", "debug"),
    ("key.debug.profiling", "key.keyboard.l", "debug"),
    ("key.debug.copyLocation", "key.keyboard.c", "debug"),
    ("key.debug.dumpVersion", "key.keyboard.v", "debug"),
    ("key.debug.profilingChart", "key.keyboard.1", "debug"),
    ("key.debug.fpsCharts", "key.keyboard.2", "debug"),
    ("key.debug.networkCharts", "key.keyboard.3", "debug"),
    ("key.debug.lightmapTexture", "key.keyboard.4", "debug"),
    ("key.debug.improvedTransparency", "key.keyboard.x", "debug"),
];
/// Mappings Options builds as `ToggleKeyMapping` (hold or toggle by option).
pub fn is_toggle(name: &str) -> bool {
    matches!(name, "key.sneak" | "key.sprint" | "key.use" | "key.attack")
}
/// KeyMapping.Category sort order.
pub const CATEGORIES: [&str; 8] = [
    "movement",
    "misc",
    "multiplayer",
    "gameplay",
    "inventory",
    "creative",
    "spectator",
    "debug",
];

/// InputConstants key names <-> winit physical keys.
const KEYS: &[(&str, KeyCode)] = &[
    ("key.keyboard.a", KeyCode::KeyA),
    ("key.keyboard.b", KeyCode::KeyB),
    ("key.keyboard.c", KeyCode::KeyC),
    ("key.keyboard.d", KeyCode::KeyD),
    ("key.keyboard.e", KeyCode::KeyE),
    ("key.keyboard.f", KeyCode::KeyF),
    ("key.keyboard.g", KeyCode::KeyG),
    ("key.keyboard.h", KeyCode::KeyH),
    ("key.keyboard.i", KeyCode::KeyI),
    ("key.keyboard.j", KeyCode::KeyJ),
    ("key.keyboard.k", KeyCode::KeyK),
    ("key.keyboard.l", KeyCode::KeyL),
    ("key.keyboard.m", KeyCode::KeyM),
    ("key.keyboard.n", KeyCode::KeyN),
    ("key.keyboard.o", KeyCode::KeyO),
    ("key.keyboard.p", KeyCode::KeyP),
    ("key.keyboard.q", KeyCode::KeyQ),
    ("key.keyboard.r", KeyCode::KeyR),
    ("key.keyboard.s", KeyCode::KeyS),
    ("key.keyboard.t", KeyCode::KeyT),
    ("key.keyboard.u", KeyCode::KeyU),
    ("key.keyboard.v", KeyCode::KeyV),
    ("key.keyboard.w", KeyCode::KeyW),
    ("key.keyboard.x", KeyCode::KeyX),
    ("key.keyboard.y", KeyCode::KeyY),
    ("key.keyboard.z", KeyCode::KeyZ),
    ("key.keyboard.1", KeyCode::Digit1),
    ("key.keyboard.2", KeyCode::Digit2),
    ("key.keyboard.3", KeyCode::Digit3),
    ("key.keyboard.4", KeyCode::Digit4),
    ("key.keyboard.5", KeyCode::Digit5),
    ("key.keyboard.6", KeyCode::Digit6),
    ("key.keyboard.7", KeyCode::Digit7),
    ("key.keyboard.8", KeyCode::Digit8),
    ("key.keyboard.9", KeyCode::Digit9),
    ("key.keyboard.0", KeyCode::Digit0),
    ("key.keyboard.enter", KeyCode::Enter),
    ("key.keyboard.escape", KeyCode::Escape),
    ("key.keyboard.backspace", KeyCode::Backspace),
    ("key.keyboard.tab", KeyCode::Tab),
    ("key.keyboard.space", KeyCode::Space),
    ("key.keyboard.minus", KeyCode::Minus),
    ("key.keyboard.equal", KeyCode::Equal),
    ("key.keyboard.left.bracket", KeyCode::BracketLeft),
    ("key.keyboard.right.bracket", KeyCode::BracketRight),
    ("key.keyboard.backslash", KeyCode::Backslash),
    ("key.keyboard.semicolon", KeyCode::Semicolon),
    ("key.keyboard.apostrophe", KeyCode::Quote),
    ("key.keyboard.grave.accent", KeyCode::Backquote),
    ("key.keyboard.comma", KeyCode::Comma),
    ("key.keyboard.period", KeyCode::Period),
    ("key.keyboard.slash", KeyCode::Slash),
    ("key.keyboard.caps.lock", KeyCode::CapsLock),
    ("key.keyboard.f1", KeyCode::F1),
    ("key.keyboard.f2", KeyCode::F2),
    ("key.keyboard.f3", KeyCode::F3),
    ("key.keyboard.f4", KeyCode::F4),
    ("key.keyboard.f5", KeyCode::F5),
    ("key.keyboard.f6", KeyCode::F6),
    ("key.keyboard.f7", KeyCode::F7),
    ("key.keyboard.f8", KeyCode::F8),
    ("key.keyboard.f9", KeyCode::F9),
    ("key.keyboard.f10", KeyCode::F10),
    ("key.keyboard.f11", KeyCode::F11),
    ("key.keyboard.f12", KeyCode::F12),
    ("key.keyboard.print.screen", KeyCode::PrintScreen),
    ("key.keyboard.scroll.lock", KeyCode::ScrollLock),
    ("key.keyboard.pause", KeyCode::Pause),
    ("key.keyboard.insert", KeyCode::Insert),
    ("key.keyboard.home", KeyCode::Home),
    ("key.keyboard.page.up", KeyCode::PageUp),
    ("key.keyboard.delete", KeyCode::Delete),
    ("key.keyboard.end", KeyCode::End),
    ("key.keyboard.page.down", KeyCode::PageDown),
    ("key.keyboard.right", KeyCode::ArrowRight),
    ("key.keyboard.left", KeyCode::ArrowLeft),
    ("key.keyboard.down", KeyCode::ArrowDown),
    ("key.keyboard.up", KeyCode::ArrowUp),
    ("key.keyboard.num.lock", KeyCode::NumLock),
    ("key.keyboard.keypad.divide", KeyCode::NumpadDivide),
    ("key.keyboard.keypad.multiply", KeyCode::NumpadMultiply),
    ("key.keyboard.keypad.subtract", KeyCode::NumpadSubtract),
    ("key.keyboard.keypad.add", KeyCode::NumpadAdd),
    ("key.keyboard.keypad.enter", KeyCode::NumpadEnter),
    ("key.keyboard.keypad.1", KeyCode::Numpad1),
    ("key.keyboard.keypad.2", KeyCode::Numpad2),
    ("key.keyboard.keypad.3", KeyCode::Numpad3),
    ("key.keyboard.keypad.4", KeyCode::Numpad4),
    ("key.keyboard.keypad.5", KeyCode::Numpad5),
    ("key.keyboard.keypad.6", KeyCode::Numpad6),
    ("key.keyboard.keypad.7", KeyCode::Numpad7),
    ("key.keyboard.keypad.8", KeyCode::Numpad8),
    ("key.keyboard.keypad.9", KeyCode::Numpad9),
    ("key.keyboard.keypad.0", KeyCode::Numpad0),
    ("key.keyboard.keypad.period", KeyCode::NumpadDecimal),
    ("key.keyboard.keypad.equal", KeyCode::NumpadEqual),
    ("key.keyboard.menu", KeyCode::ContextMenu),
    ("key.keyboard.left.control", KeyCode::ControlLeft),
    ("key.keyboard.left.shift", KeyCode::ShiftLeft),
    ("key.keyboard.left.alt", KeyCode::AltLeft),
    ("key.keyboard.left.win", KeyCode::SuperLeft),
    ("key.keyboard.right.control", KeyCode::ControlRight),
    ("key.keyboard.right.shift", KeyCode::ShiftRight),
    ("key.keyboard.right.alt", KeyCode::AltRight),
    ("key.keyboard.right.win", KeyCode::SuperRight),
];
pub fn key_code(name: &str) -> Option<KeyCode> {
    KEYS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}
pub fn key_name(code: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|(_, k)| *k == code).map(|(n, _)| *n)
}
pub fn mouse_name(button: MouseButton) -> String {
    match button {
        MouseButton::Left => "key.mouse.left".into(),
        MouseButton::Right => "key.mouse.right".into(),
        MouseButton::Middle => "key.mouse.middle".into(),
        MouseButton::Back => "key.mouse.4".into(),
        MouseButton::Forward => "key.mouse.5".into(),
        MouseButton::Other(n) => format!("key.mouse.{}", n + 1),
    }
}
pub fn mouse_button(name: &str) -> Option<MouseButton> {
    Some(match name {
        "key.mouse.left" => MouseButton::Left,
        "key.mouse.right" => MouseButton::Right,
        "key.mouse.middle" => MouseButton::Middle,
        "key.mouse.4" => MouseButton::Back,
        "key.mouse.5" => MouseButton::Forward,
        _ => return None,
    })
}
/// InputConstants.Key.getDisplayName: one-character keys as the upper-case
/// character, others by translation.
pub fn display_name(key: &str) -> Text {
    if let Some(rest) = key.strip_prefix("key.keyboard.")
        && rest.chars().count() == 1
    {
        let c = rest.to_uppercase();
        return Text::literal(c);
    }
    let symbol = match key {
        "key.keyboard.minus" => Some("-"),
        "key.keyboard.equal" => Some("="),
        "key.keyboard.left.bracket" => Some("["),
        "key.keyboard.right.bracket" => Some("]"),
        "key.keyboard.backslash" => Some("\\"),
        "key.keyboard.semicolon" => Some(";"),
        "key.keyboard.apostrophe" => Some("'"),
        "key.keyboard.grave.accent" => Some("`"),
        "key.keyboard.comma" => Some(","),
        "key.keyboard.period" => Some("."),
        "key.keyboard.slash" => Some("/"),
        _ => None,
    };
    if let Some(s) = symbol {
        return Text::literal(s);
    }
    if let Some(n) = key.strip_prefix("key.mouse.")
        && n.parse::<u32>().is_ok()
        && !crate::lang::has(key)
    {
        return Text::translate("key.mouse", &[Text::literal(n)]);
    }
    Text::translate(key, &[])
}

/// A keybind a mod registered (capability `input`): listed in Controls under
/// the mod's name, rebound and saved like vanilla's (in `MOD_KEYS_FILE`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModKey {
    /// `key.<mod id>.<bind>`.
    pub name: &'static str,
    pub label: String,
    /// The mod's name, its category in Controls.
    pub category: String,
    pub default: String,
}
/// Mod keybinds' saved keys, beside options.txt: `key_<name>:<key>` lines.
pub const MOD_KEYS_FILE: &str = "horizon-mod-keys.txt";

/// Current bindings by mapping name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBinds {
    keys: BTreeMap<&'static str, String>,
    /// Mods' keybinds, in registration order.
    mods: Vec<ModKey>,
    /// Saved mod keybinds, including mods not running now (kept on save).
    saved_mods: BTreeMap<String, String>,
}
impl Default for KeyBinds {
    fn default() -> Self {
        Self {
            keys: MAPPINGS
                .iter()
                .map(|(n, d, _)| (*n, (*d).to_owned()))
                .collect(),
            mods: Vec::new(),
            saved_mods: BTreeMap::new(),
        }
    }
}
impl KeyBinds {
    pub fn get(&self, name: &str) -> &str {
        self.keys
            .get(name)
            .map_or("key.keyboard.unknown", String::as_str)
    }
    pub fn default_of(name: &str) -> &'static str {
        MAPPINGS
            .iter()
            .find(|m| m.0 == name)
            .map_or("key.keyboard.unknown", |m| m.1)
    }
    /// A vanilla or mod mapping's default key.
    pub fn default_for(&self, name: &str) -> &str {
        match self.mods.iter().find(|m| m.name == name) {
            Some(m) => &m.default,
            None => Self::default_of(name),
        }
    }
    pub fn is_default(&self, name: &str) -> bool {
        self.get(name) == self.default_for(name)
    }
    pub fn set(&mut self, name: &str, key: &str) {
        if let Some((n, _, _)) = MAPPINGS.iter().find(|m| m.0 == name) {
            self.keys.insert(n, key.to_owned());
        } else if let Some(m) = self.mods.iter().find(|m| m.name == name) {
            self.keys.insert(m.name, key.to_owned());
            self.saved_mods.insert(m.name.to_owned(), key.to_owned());
        }
    }
    /// Every mapping's name: vanilla's, then the mods'.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        MAPPINGS
            .iter()
            .map(|m| m.0)
            .chain(self.mods.iter().map(|m| m.name))
    }
    pub fn mod_keys(&self) -> &[ModKey] {
        &self.mods
    }
    /// A mod's keybind (`key.<mod id>.<bind>`): bound to its saved key, or
    /// `default`. Registering it again updates its label and default.
    pub fn register_mod_key(&mut self, name: &str, label: &str, category: &str, default: &str) {
        if let Some(m) = self.mods.iter_mut().find(|m| m.name == name) {
            m.label = label.to_owned();
            m.category = category.to_owned();
            m.default = default.to_owned();
            return;
        }
        // Names live as long as the game (Controls rows hold them); a mod
        // re-registering after a reload reuses its entry above.
        let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        let key = self
            .saved_mods
            .get(name)
            .cloned()
            .unwrap_or_else(|| default.to_owned());
        self.keys.insert(name, key);
        self.mods.push(ModKey {
            name,
            label: label.to_owned(),
            category: category.to_owned(),
            default: default.to_owned(),
        });
    }
    /// Takes `other`'s mod keybinds and their bindings (settings rebuilt
    /// from options.txt, which does not hold them).
    pub fn adopt_mod_keys(&mut self, other: &KeyBinds) {
        self.saved_mods = other.saved_mods.clone();
        self.mods = other.mods.clone();
        for m in &self.mods {
            self.keys.insert(m.name, other.get(m.name).to_owned());
        }
    }
    /// Reads `MOD_KEYS_FILE` in `dir`.
    pub fn load_mod_keys(&mut self, dir: &std::path::Path) {
        let Ok(text) = std::fs::read_to_string(dir.join(MOD_KEYS_FILE)) else {
            return;
        };
        for line in text.lines() {
            if let Some((name, key)) = line.strip_prefix("key_").and_then(|l| l.split_once(':')) {
                self.saved_mods.insert(name.to_owned(), key.to_owned());
            }
        }
    }
    /// Writes `MOD_KEYS_FILE` in `dir` (only once a mod keybind exists).
    pub fn save_mod_keys(&self, dir: &std::path::Path) {
        let path = dir.join(MOD_KEYS_FILE);
        if self.saved_mods.is_empty() && self.mods.is_empty() && !path.exists() {
            return;
        }
        let mut all = self.saved_mods.clone();
        for m in &self.mods {
            all.insert(m.name.to_owned(), self.get(m.name).to_owned());
        }
        let text: String = all.iter().map(|(n, k)| format!("key_{n}:{k}\n")).collect();
        let _ = std::fs::write(path, text);
    }
    /// Load `key_<name>` from options.txt; false for unknown mappings.
    pub fn set_saved(&mut self, name: &str, key: &str) -> bool {
        if MAPPINGS.iter().any(|m| m.0 == name) {
            self.set(name, key);
            true
        } else {
            false
        }
    }
    pub fn saved(&self) -> Vec<(&'static str, String)> {
        MAPPINGS
            .iter()
            .map(|(n, _, _)| (*n, self.get(n).to_owned()))
            .collect()
    }
    pub fn reset_all(&mut self) {
        let names: Vec<&'static str> = self.names().collect();
        for name in names {
            let default = self.default_for(name).to_owned();
            self.set(name, &default);
        }
    }
    /// Mappings sharing `name`'s key (KeyMapping.same), for conflict marks.
    pub fn conflicts(&self, name: &str) -> Vec<&'static str> {
        let key = self.get(name);
        if key == "key.keyboard.unknown" {
            return Vec::new();
        }
        self.names()
            .filter(|n| {
                *n != name && self.get(n) == key && (!self.is_default(n) || !self.is_default(name))
            })
            .collect()
    }
    /// The mapping a physical input triggers (first in list order).
    pub fn mapping_for(&self, key: &str) -> Option<&'static str> {
        MAPPINGS.iter().map(|m| m.0).find(|n| self.get(n) == key)
    }
    /// Translate a physical key into the default key of the mapping it is
    /// bound to, so game code written against vanilla defaults follows the
    /// player's bindings. `None` means the key now does nothing; mouse
    /// mappings bound to keys come back as `Err(button)`.
    pub fn translate(&self, key: KeyCode) -> Option<Result<KeyCode, MouseButton>> {
        let Some(name) = key_name(key) else {
            return Some(Ok(key));
        };
        if let Some(mapping) = self.mapping_for(name) {
            let default = Self::default_of(mapping);
            if let Some(code) = key_code(default) {
                return Some(Ok(code));
            }
            return mouse_button(default).map(Err);
        }
        // An unbound key whose default meaning was moved elsewhere is inert.
        if MAPPINGS
            .iter()
            .any(|(n, d, _)| *d == name && !self.is_default(n))
        {
            return None;
        }
        Some(Ok(key))
    }
    /// Translate a mouse button the same way (keys bound to mouse buttons).
    pub fn translate_mouse(&self, button: MouseButton) -> Option<Result<KeyCode, MouseButton>> {
        let name = mouse_name(button);
        if let Some(mapping) = self.mapping_for(&name) {
            let default = Self::default_of(mapping);
            if let Some(b) = mouse_button(default) {
                return Some(Err(b));
            }
            return key_code(default).map(Ok);
        }
        if MAPPINGS
            .iter()
            .any(|(n, d, _)| *d == name && !self.is_default(n))
        {
            return None;
        }
        Some(Err(button))
    }
    /// `AbstractContainerScreen.checkHotbarMouseClicked`: off hand, then hotbar.
    pub fn container_mouse_key(&self, button: MouseButton, creative: bool) -> Option<KeyCode> {
        let name = mouse_name(button);
        if matches!(button, MouseButton::Left | MouseButton::Right)
            || (creative && self.get("key.pickItem") == name)
        {
            return None;
        }
        std::iter::once("key.swapOffhand")
            .chain(
                MAPPINGS
                    .iter()
                    .map(|m| m.0)
                    .filter(|n| n.starts_with("key.hotbar.")),
            )
            .find(|mapping| self.get(mapping) == name)
            .and_then(|mapping| key_code(Self::default_of(mapping)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn container_mouse_hotkeys_follow_slot_bindings_and_pick_item_priority() {
        for button in [MouseButton::Back, MouseButton::Forward, MouseButton::Middle] {
            for (mapping, default, _) in MAPPINGS.iter().filter(|m| m.0.starts_with("key.hotbar."))
            {
                let mut keys = KeyBinds::default();
                keys.set(mapping, &mouse_name(button));
                assert_eq!(keys.container_mouse_key(button, false), key_code(default));
                assert_eq!(
                    keys.container_mouse_key(button, true),
                    if button == MouseButton::Middle {
                        None
                    } else {
                        key_code(default)
                    }
                );
            }
        }
        let mut keys = KeyBinds::default();
        keys.set("key.hotbar.1", "key.mouse.4");
        keys.set("key.swapOffhand", "key.mouse.4");
        assert_eq!(
            keys.container_mouse_key(MouseButton::Back, false),
            Some(KeyCode::KeyF)
        );
        keys.set("key.hotbar.2", "key.mouse.left");
        assert_eq!(keys.container_mouse_key(MouseButton::Left, false), None);
        assert_eq!(keys.container_mouse_key(MouseButton::Right, false), None);
        assert_eq!(keys.container_mouse_key(MouseButton::Forward, false), None);
    }

    #[test]
    fn rebinding_translates_to_defaults() {
        let mut k = KeyBinds::default();
        assert_eq!(k.translate(KeyCode::KeyW), Some(Ok(KeyCode::KeyW)));
        // Forward on the up arrow, inventory on W.
        k.set("key.forward", "key.keyboard.up");
        k.set("key.inventory", "key.keyboard.w");
        assert_eq!(k.translate(KeyCode::ArrowUp), Some(Ok(KeyCode::KeyW)));
        assert_eq!(k.translate(KeyCode::KeyW), Some(Ok(KeyCode::KeyE)));
        // E's default meaning moved and nothing else uses it.
        assert_eq!(k.translate(KeyCode::KeyE), None);
        // Attack on a key.
        k.set("key.attack", "key.keyboard.r");
        assert_eq!(k.translate(KeyCode::KeyR), Some(Err(MouseButton::Left)));
        assert_eq!(k.translate_mouse(MouseButton::Left), None);
        assert_eq!(k.translate(KeyCode::Escape), Some(Ok(KeyCode::Escape)));
        assert!(k.conflicts("key.forward").is_empty());
        k.set("key.jump", "key.keyboard.up");
        assert_eq!(k.conflicts("key.forward"), ["key.jump"]);
    }
    #[test]
    fn display_names() {
        assert_eq!(display_name("key.keyboard.w").plain(), "W");
        assert_eq!(display_name("key.keyboard.slash").plain(), "/");
    }
}
