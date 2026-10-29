//! Hook points for native mods (docs/native-mods.md, "Hook points"): empty
//! functions the game calls at fixed places, for horizon-mixins bundles to
//! hook with HEAD. Hooks that answer take an out parameter (`claimed`,
//! `zoom`…) so several mods can hook the same point. A native mod keeps its
//! own state in a module of its bundle; the helpers below read and draw what
//! Horizon's own HUD extras do, and keep a mod's settings where the
//! WebAssembly mod of the same id kept them.
//!
//! API 2 adds the item panel beside container screens and its recipe pages
//! (`native_panel_items`, `native_panel_recipes`, `native_panel_event`),
//! and the game's items and recipes (`native_recipes`,
//! `native_known_recipes`, `native_place_recipe`, `native_give`): what a
//! recipe viewer needs.
//!
//! API 3 adds what the crosshair is on (`native_target`): what a block or
//! entity info tooltip needs.
//!
//! API 4 adds any loaded block (`native_block`): its name, state and light,
//! what a light level overlay or a world map reads.
//!
//! API 5 adds the player's inventory (`native_inventory`): what an armour
//! HUD or a durability warning reads.
//!
//! API 6 adds the player's effects (`native_effects`): what a potion timer
//! reads.
//!
//! API 7 adds the player's health, food, air and experience
//! (`native_vitals`): what a food or health HUD reads.
//!
//! API 8 adds the biome at a position (`native_biome`): what a world map or
//! a coordinates HUD reads.
//!
//! API 9 adds the world's clock and weather (`native_time`): what a clock
//! or a weather HUD reads.
//!
//! API 10 adds the mobs and other players near the player
//! (`native_entities`): what a radar or a mob minimap reads.
//!
//! API 11 adds the player list (`native_players`): what a better tab list
//! or a friends list reads.
//!
//! API 12 adds the open screen and its menu's slots (`native_screen`): what
//! an inventory sorter or a chest search reads.
//!
//! API 13 adds which world the player is in (`native_world`): single player
//! by name or a server by address, what per-world waypoints key on.
//!
//! API 14 adds the scoreboard sidebar and the boss bars as the HUD shows
//! them (`native_sidebar`, `native_boss_bars`): what a HUD customizer reads.
//!
//! API 15 adds the hooks `native_join` and `native_leave`, when the window
//! enters and leaves a world: what per-world waypoints or stats load and
//! save on.
//!
//! API 16 adds the hooks `native_chat_received` and `native_chat_send`, to
//! hide a line arriving in chat or cancel one the player sends: what a chat
//! filter, a chat log or chat macros need.
//!
//! API 17 adds the hook `native_screen_changed`, when a screen or a
//! server's menu opens or closes in a world: what inventory tweaks start
//! and stop on.
//!
//! API 18 adds the hook `native_tooltip`, to add lines under an item's
//! tooltip: what a food, stats or durability tooltip needs.
//!
//! API 19 adds the hooks `native_attack`, `native_use` and `native_break`,
//! when the player clicks to attack or use and when a block breaks: what an
//! auto tool, a combat HUD or a mining counter starts on.
//!
//! API 20 adds the hook `native_sound`, told each sound that starts, and the
//! helper `native_sound_volume`, to scale or mute a sound event: what sound
//! filters and subtitles need.
//!
//! Names, parameters and call sites are kept from release to release, and
//! hook points are only ever added: each addition raises [`NATIVE_HOOKS_API`].
//! A bundle that hooks only these names the API it needs (`hooks`), and the
//! launcher builds it on any Horizon whose API is at least that one. A change
//! that would break such a bundle is not made.
//! Bodies have no `cfg` and no macros: horizon-mixins refuses a target with
//! either (`tests::hook_points_stay_hookable`).
use super::{Game, Screen, execution_role::ExecutionRole, native_panel_recipes::RecipePage};
use horizon_client::{
    chat::ChatSource,
    extras::Corner,
    extras_hud::Corners,
    hud::{HudBatch, rgba},
    hud_overlays,
    items::ItemStack,
    keybinds,
    mod_list::{self, Installed, NativePage},
    tab_list::{self, TabPlayer},
    text::Text,
};
use horizon_world::registry::BlockStateId;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use winit::{event::MouseButton, keyboard::KeyCode};

/// The hook points' API version. The launcher reads this line from the
/// source it builds; keep its form.
#[allow(dead_code)]
pub(crate) const NATIVE_HOOKS_API: u32 = 20;

/// The zoom a native mod asks for (`native_zoom`): the factor (1 is none),
/// eased through the field of view like the spyglass when `smooth`, and the
/// scale of mouse turning.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeZoom {
    pub(crate) factor: f32,
    pub(crate) smooth: bool,
    pub(crate) turn: f32,
}

/// The minimap a native mod shows (`native_minimap`), drawn by Horizon's
/// minimap: its corner, its side in GUI pixels, blocks per map pixel,
/// turning with the player or north up, round or square.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct NativeMinimap {
    pub(crate) corner: Corner,
    pub(crate) size: f32,
    pub(crate) blocks_per_pixel: f32,
    pub(crate) rotate: bool,
    pub(crate) round: bool,
}

/// The player as a native mod reads it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativePlayer {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) z: f64,
    /// Degrees; 0 faces south, 90 west (`Direction.fromYRot`).
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    /// `minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`.
    pub(crate) dimension: &'static str,
}

/// What the crosshair is on (`native_target`), as the game picks it for
/// attacking and using.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeTarget {
    /// A block's position, the face hit as a unit vector (`[0, 1, 0]` is
    /// the top), its name (`minecraft:wheat`) and its state's properties
    /// (`age` = `7`), sorted by name.
    Block {
        pos: [i32; 3],
        face: [i32; 3],
        block: String,
        properties: Vec<(String, String)>,
    },
    /// An entity before any block: its id (`Entity.getId`; none for a
    /// player, a projectile or the dragon) and, for a mob, its type
    /// (`minecraft:zombie`), custom name and health out of its type's.
    Entity {
        id: Option<i32>,
        kind: Option<String>,
        name: Option<String>,
        health: Option<[f32; 2]>,
    },
}

/// A loaded block (`native_block`): its name (`minecraft:wheat`), its
/// state's properties (`age` = `7`) sorted by name, and its light levels,
/// 0 to 15: from blocks, and from the sky before night darkens it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeBlock {
    pub(crate) block: String,
    pub(crate) properties: Vec<(String, String)>,
    pub(crate) block_light: u8,
    pub(crate) sky_light: u8,
}

/// A stack of the player's inventory (`native_inventory`), with its damage
/// out of its item's durability; `max_damage` is 0 for a stack that never
/// breaks (no durability, or unbreakable).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeSlot {
    pub(crate) stack: NativeStack,
    pub(crate) damage: u32,
    pub(crate) max_damage: u32,
}

/// The player's inventory (`native_inventory`), by vanilla's `Inventory`
/// slots: 0 to 8 the hotbar, 9 to 35 the rest, 36 to 39 the armour from
/// feet to head, 40 the off hand; `selected` is the hotbar slot in hand.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeInventory {
    pub(crate) slots: Vec<Option<NativeSlot>>,
    pub(crate) selected: u32,
}

/// The open screen (`native_screen`). `screen` is Horizon's: `playing`,
/// `inventory`, `chat`, `paused`, `creative`... `menu` is the server's menu
/// type (`minecraft:generic_9x3`), none for the player's own inventory or
/// off a server. `slots` are the menu's in vanilla's order (the container's,
/// then the inventory's 27, then the hotbar), empty where Horizon's menu
/// engine doesn't model the screen.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeScreen {
    pub(crate) screen: &'static str,
    pub(crate) menu: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) slots: Vec<Option<NativeSlot>>,
}

/// The world the player is in (`native_world`): a single player world, by
/// its `name`, `published` when open to LAN or friends; or a server, by its
/// `address` as Horizon shows it (friend codes hidden).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeWorld {
    pub(crate) single_player: bool,
    pub(crate) published: bool,
    pub(crate) name: Option<String>,
    pub(crate) address: Option<String>,
}

/// The scoreboard sidebar (`native_sidebar`): its title and its lines top
/// first, each the name as its team shows it and the score as formatted
/// (empty for a blank format). At most 15, as the HUD draws.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeSidebar {
    pub(crate) title: String,
    pub(crate) lines: Vec<(String, String)>,
}

/// A boss bar (`native_boss_bars`): its name, progress from 0 to 1, color
/// (`pink`, `blue`, `red`, `green`, `yellow`, `purple`, `white`) and
/// notches (0 for a plain bar, else 6, 10, 12 or 20).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeBossBar {
    pub(crate) name: String,
    pub(crate) progress: f32,
    pub(crate) color: &'static str,
    pub(crate) notches: u8,
}

/// An effect on the player (`native_effects`): `minecraft:speed`, its
/// amplifier (0 is level I), its ticks left (-1 is infinite), from a beacon
/// or conduit (`ambient`), and whether it shows particles.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeEffect {
    pub(crate) id: String,
    pub(crate) amplifier: i32,
    pub(crate) duration: i32,
    pub(crate) ambient: bool,
    pub(crate) particles: bool,
}

/// The player's health, food, air and experience (`native_vitals`), as the
/// HUD draws them. Food exhaustion is left out: servers don't send it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeVitals {
    pub(crate) health: f32,
    pub(crate) max_health: f32,
    pub(crate) absorption: f32,
    /// The `minecraft:armor` attribute: half a chestplate icon each.
    pub(crate) armor: i32,
    /// 0 to 20.
    pub(crate) food: i32,
    pub(crate) saturation: f32,
    /// Ticks of breath, 300 when full.
    pub(crate) air: i32,
    pub(crate) xp_level: i32,
    /// 0 to 1 toward the next level.
    pub(crate) xp_progress: f32,
    pub(crate) xp_total: i32,
    /// `survival`, `creative`, `adventure`, `spectator`.
    pub(crate) mode: &'static str,
    pub(crate) hardcore: bool,
}

/// A mob or another player near the player (`native_entities`).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeEntity {
    /// `Entity.getId`, as `NativeTarget::Entity` gives it.
    pub(crate) id: u32,
    /// `minecraft:zombie`, `minecraft:player`.
    pub(crate) kind: String,
    /// Its feet.
    pub(crate) pos: [f64; 3],
    /// A player's profile name, else a custom name.
    pub(crate) name: Option<String>,
    /// Health and max health.
    pub(crate) health: [f32; 2],
}

/// An entry of the player list (`native_players`), as Tab shows it.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeListPlayer {
    pub(crate) uuid: u128,
    /// The profile name.
    pub(crate) name: String,
    /// The name as the list draws it: the server's display name, else the
    /// profile name with its team's prefix and suffix.
    pub(crate) display: String,
    /// `survival`, `creative`, `adventure`, `spectator`.
    pub(crate) mode: &'static str,
    /// Milliseconds; negative when unknown.
    pub(crate) latency: i32,
}

/// The world's clock and weather (`native_time`), as the server sent them.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeTime {
    /// Ticks since the world was made; never stops.
    pub(crate) game_time: i64,
    /// Days of the world's clock (`/time query day`).
    pub(crate) day: i64,
    /// 0 to 23999: 0 is sunrise, 6000 noon, 18000 midnight.
    pub(crate) time_of_day: i64,
    /// 0 to 1, fading in and out.
    pub(crate) rain: f32,
    /// 0 to 1, scaled by `rain`.
    pub(crate) thunder: f32,
}

/// A native mod as Options > Mods lists it (`native_mods`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeMod {
    /// Its id: its settings folder and its keys' lines.
    pub(crate) id: String,
    /// `Name 1.0`.
    pub(crate) title: String,
    pub(crate) description: String,
    /// Its keys: name, label and default key (`key.keyboard.z`). The page
    /// lists them first; `native_mod_key` reads the player's choice.
    pub(crate) keys: Vec<[String; 3]>,
}

/// An item the panel shows (`native_panel_items`): its id
/// (`minecraft:diamond`), a count (1 draws none) and its components beyond
/// the item's own, in vanilla's item syntax, or "".
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeStack {
    pub(crate) item: String,
    pub(crate) count: u32,
    pub(crate) components: String,
}

/// How the panel lays out a recipe of `native_panel_recipes`.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeRecipeLayout {
    /// A crafting grid of `width` x `height` slots, an arrow and the result.
    Grid,
    /// One input over a flame, an arrow and the result (furnaces, smokers,
    /// campfires).
    Cooking,
    /// One input, an arrow and the result (the stonecutter).
    Single,
    /// Template, base and addition, an arrow and the result.
    Smithing,
    /// A potion and an ingredient, an arrow and the result.
    Brewing,
}

/// A recipe the panel shows (`native_panel_recipes`). Each slot, and the
/// result, cycles through its stacks, one a second; an empty slot shows
/// nothing.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeRecipeView {
    pub(crate) layout: NativeRecipeLayout,
    /// The grid's size (`Grid` only), at most 3 x 3.
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) slots: Vec<Vec<NativeStack>>,
    pub(crate) results: Vec<NativeStack>,
    /// A short line under the recipe (a cooking time), or "".
    pub(crate) note: String,
    /// The action button beside it: none, enabled, or shown greyed.
    pub(crate) action: Option<bool>,
}

/// What the player did on a native mod's panel (`native_panel_event`).
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativePanelEvent {
    /// A click on an item of the panel, by its index in
    /// `native_panel_items`' list.
    Clicked { index: u32, right: bool },
    /// The search box's text, each time the player changes it.
    Search(String),
    /// A key (`key.keyboard.r`; compare it with `native_mod_key`) went down
    /// while the pointer was on an item: a slot of the screen's, or the
    /// panel's.
    Key { key: String, stack: NativeStack },
    /// A click on a slot of a recipe the panel shows: the recipe's index in
    /// `native_panel_recipes`' list and the slot's in it (the results come
    /// after the slots), and the stack it showed then.
    SlotClicked {
        recipe: u32,
        slot: u32,
        stack: NativeStack,
        right: bool,
    },
    /// The action button of the recipe at this index.
    Action(u32),
    /// The player turned the recipes' page: the index of the first recipe
    /// now shown.
    Page(u32),
    /// The recipe page's back button.
    Back,
    /// The player picked the tab at this index.
    Tab(u32),
}

/// An item of [`NativeRecipes::items`], by its index there, with a count
/// and the components it has beyond the item's own, in vanilla's item
/// syntax (`[minecraft:potion_contents={potion:"minecraft:swiftness"}]`),
/// or "".
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeRecipeStack {
    pub(crate) item: u32,
    pub(crate) count: u32,
    pub(crate) components: String,
}

/// The station a recipe is made at.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeRecipeKind {
    CraftingShaped,
    CraftingShapeless,
    Smelting,
    Blasting,
    Smoking,
    Campfire,
    Stonecutting,
    Smithing,
    Brewing,
}

/// A recipe of [`NativeRecipes`].
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NativeRecipe {
    /// `minecraft:oak_planks`.
    pub(crate) id: String,
    pub(crate) kind: NativeRecipeKind,
    /// The recipe book's tab (`crafting_building_blocks`, `furnace_food`),
    /// or "".
    pub(crate) category: String,
    /// Recipes the book shows on one button share a group, or "".
    pub(crate) group: String,
    /// A shaped recipe's grid: `slots` holds its cells row by row. 0 for
    /// the other kinds.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Each slot's stacks, any one of which fits; an empty list is an empty
    /// cell. Cooking and stonecutting: the input. Smithing: the template,
    /// the base, the addition. Brewing: the bottle, the reagent.
    pub(crate) slots: Vec<Vec<NativeRecipeStack>>,
    /// What it makes; several when that varies (a trim on any armour).
    pub(crate) results: Vec<NativeRecipeStack>,
    /// Cooking: game ticks and experience; 0 for the other kinds.
    pub(crate) cooking_time: u32,
    pub(crate) experience: f32,
    /// The recipe book's id for it (one per way the book shows it), as
    /// `native_known_recipes` lists them; -1 for the kinds the book doesn't
    /// show (campfire, stonecutting, smithing, brewing).
    pub(crate) book_id: i32,
}

/// An item of [`NativeRecipes::items`].
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeItem {
    /// `minecraft:diamond`, `<mod>:<path>`.
    pub(crate) id: String,
    /// In the player's language.
    pub(crate) name: String,
    /// Its item tags, without the `#`.
    pub(crate) tags: Vec<String>,
}

/// Every item and recipe the game knows (`native_recipes`): `items` in the
/// registry's order, which each [`NativeRecipeStack::item`] indexes; the
/// recipes are the book's in its order, then campfire, stonecutting,
/// smithing and brewing.
#[allow(dead_code)]
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct NativeRecipes {
    pub(crate) items: Vec<NativeItem>,
    pub(crate) recipes: Vec<NativeRecipe>,
}

/// The mappings `native_mapping` answers for, with the key their default
/// binding is (none: a mouse button, attack or use).
#[allow(dead_code)]
const MAPPINGS: [(&str, Option<KeyCode>); 9] = [
    ("key.forward", Some(KeyCode::KeyW)),
    ("key.left", Some(KeyCode::KeyA)),
    ("key.back", Some(KeyCode::KeyS)),
    ("key.right", Some(KeyCode::KeyD)),
    ("key.jump", Some(KeyCode::Space)),
    ("key.sneak", Some(KeyCode::ShiftLeft)),
    ("key.sprint", Some(KeyCode::ControlLeft)),
    ("key.attack", None),
    ("key.use", None),
];

/// The hook points. Each does nothing in Horizon.
impl Game<'_> {
    /// Once a frame, in a world or not, after Horizon's extras.
    pub(super) fn native_frame(&mut self, elapsed: Duration) {
        let _ = elapsed;
    }
    /// Once a tick, in every `Game`: the window's (in single player, on LAN
    /// and on servers), and the server's own (`native_window` is false
    /// there: the single player server thread, a dedicated server).
    pub(super) fn native_tick(&mut self) {}
    /// The HUD (not drawn under F1), after the minimap: place a box with
    /// `corners.place` (boxes stack away from their edge) and draw it with
    /// `native_fill` and `native_text`. Skip a corner `native_corner_shown`
    /// says is taken.
    pub(super) fn native_hud(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        let _ = (batch, corners);
    }
    /// A key or mouse button went down or up, by its vanilla name
    /// (`key.keyboard.z`, `key.mouse.left`): keys on any screen (check
    /// `native_playing`), mouse buttons while playing. Repeats are left out.
    pub(super) fn native_key(&mut self, key: &str, pressed: bool) {
        let _ = (key, pressed);
    }
    /// Wheel steps while playing: set `claimed` to keep them from the hotbar.
    pub(super) fn native_wheel(&mut self, steps: f32, claimed: &mut bool) {
        let _ = (steps, claimed);
    }
    /// A command typed or clicked in chat, without its slash, before the
    /// server and Horizon's commands see it: set `ran` when the mod ran it.
    pub(super) fn native_command(&mut self, command: &str, ran: &mut bool) {
        let _ = (command, ran);
    }
    /// The zoom a native mod asks for, applied while playing (the spyglass's
    /// own view wins): set `zoom` when it is none yet.
    pub(super) fn native_zoom(&self, zoom: &mut Option<NativeZoom>) {
        let _ = zoom;
    }
    /// The minimap a native mod shows: set `minimap` when it is none yet.
    pub(super) fn native_minimap(&self, minimap: &mut Option<NativeMinimap>) {
        let _ = minimap;
    }
    /// Options > Mods lists the native mods: push yours.
    pub(super) fn native_mods(&mut self, mods: &mut Vec<NativeMod>) {
        let _ = mods;
    }
    /// The buttons of mod `mod_id`'s settings page, under its keys: push
    /// their labels when it is yours. Asked again each time the page opens
    /// and after each click.
    pub(super) fn native_settings(&mut self, mod_id: &str, buttons: &mut Vec<String>) {
        let _ = (mod_id, buttons);
    }
    /// Button `button` of mod `mod_id`'s settings page was clicked.
    pub(super) fn native_settings_click(&mut self, mod_id: &str, button: usize) {
        let _ = (mod_id, button);
    }
    /// The player did `event` on mod `mod_id`'s panel (`native_panel_items`):
    /// act on it when it is yours. API 2.
    pub(super) fn native_panel_event(&mut self, mod_id: &str, event: &NativePanelEvent) {
        let _ = (mod_id, event);
    }
    /// The window entered `world`: a server let the player in (single
    /// player's own server included), or a world opened in the frame. The
    /// player may not be placed yet. API 15.
    pub(super) fn native_join(&mut self, world: &NativeWorld) {
        let _ = world;
    }
    /// The window is leaving the world it joined, before any of it goes:
    /// `native_world` still reads it. API 15.
    pub(super) fn native_leave(&mut self) {}
    /// A line reached chat, as plain text (a server's or a player's, the
    /// player's own included), past the Chat option's filter: set `hide` to
    /// keep it off the chat. API 16.
    pub(super) fn native_chat_received(&mut self, text: &str, hide: &mut bool) {
        let _ = (text, hide);
    }
    /// The player sends `text` from the chat box, not a command
    /// (`native_command` has those): set `cancel` to keep it from the
    /// server and the chat. API 16.
    pub(super) fn native_chat_send(&mut self, text: &str, cancel: &mut bool) {
        let _ = (text, cancel);
    }
    /// In a world, the open screen became `screen` with the server's `menu`
    /// (as `native_screen` names them; `playing` when one closed), seen
    /// once a frame. Not told on joining. API 17.
    pub(super) fn native_screen_changed(&mut self, screen: &str, menu: Option<&str>) {
        let _ = (screen, menu);
    }
    /// An item's tooltip shows for `stack`, each frame it is drawn: push
    /// lines (`§` codes allowed) with their colour to add under Horizon's.
    /// Not asked when the stack hides its tooltip. API 18.
    pub(super) fn native_tooltip(&self, stack: &NativeSlot, lines: &mut Vec<(String, u32)>) {
        let _ = (stack, lines);
    }
    /// The player pressed attack while playing, on `target` (as
    /// `native_target` gives it; none in the air), before the game acts on
    /// the click. Holding the button down is not told again. API 19.
    pub(super) fn native_attack(&mut self, target: Option<&NativeTarget>) {
        let _ = target;
    }
    /// The player pressed use while playing, on `target` as for
    /// `native_attack`. Using again while the button is held is not told.
    /// API 19.
    pub(super) fn native_use(&mut self, target: Option<&NativeTarget>) {
        let _ = target;
    }
    /// The player broke `block` (`minecraft:stone`) at `pos`, as this window
    /// sees it go (on a server, before the server confirms). API 19.
    pub(super) fn native_break(&mut self, pos: [i32; 3], block: &str) {
        let _ = (pos, block);
    }
    /// Sound event `id` (`minecraft:block.chest.open`) started this frame at
    /// `pos` (none for one played around the listener, like a click) and
    /// `volume`, after `native_sound_volume`. Muted sounds are not told.
    /// API 20.
    pub(super) fn native_sound(&mut self, id: &str, pos: Option<[f64; 3]>, volume: f32) {
        let _ = (id, pos, volume);
    }
}

/// Helpers for native mods, kept as stable as the hook points.
// Native mods use these; nothing in Horizon does.
#[allow(dead_code)]
impl Game<'_> {
    /// Whether the player left mod `mod_id` on in Options > Mods. A mod
    /// turned off does nothing (check this first in each hook) until the
    /// launcher's next build leaves it out.
    pub(super) fn native_on(&self, mod_id: &str) -> bool {
        !self.settings.mods.disabled.contains(mod_id)
    }
    /// Playing, with nothing over the world: keys and zoom apply.
    pub(super) fn native_playing(&self) -> bool {
        self.screen == Screen::Playing
            && self.title.is_none()
            && self.sign_edit.is_none()
            && self.book.is_none()
    }
    /// This `Game` is the window's, not a server's.
    pub(super) fn native_window(&self) -> bool {
        self.options.execution_role == ExecutionRole::Client
    }
    /// Whether a box in `corner` shows: F3's debug screen takes both top
    /// corners (its left and right columns), and a bottom corner while one
    /// of its charts is drawn there.
    pub(super) fn native_corner_shown(&self, corner: Corner) -> bool {
        let charts = self.debug_charts.corners(!self.has_singleplayer_server());
        !debug_takes(corner, self.debug, charts)
    }
    /// The width of `text` in GUI pixels.
    pub(super) fn native_text_width(&self, text: &str) -> f32 {
        HudBatch::text_width(&self.gui.atlas, text)
    }
    /// A rectangle in GUI pixels, `argb` (0x78000000 is the extras' box).
    pub(super) fn native_fill(&self, batch: &mut HudBatch, rect: [f32; 4], argb: u32) {
        let [x, y, w, h] = rect;
        let [a, r, g, b] = argb.to_be_bytes();
        batch.fill(&self.gui.atlas, x, y, w, h, rgba(r, g, b, a));
    }
    /// A line of text at `(x, y)` in GUI pixels, opaque `rgb`.
    pub(super) fn native_text(
        &self,
        batch: &mut HudBatch,
        text: &str,
        at: [f32; 2],
        rgb: u32,
        shadow: bool,
    ) {
        let [x, y] = at;
        let [_, r, g, b] = rgb.to_be_bytes();
        let color = rgba(r, g, b, 0xFF);
        batch.text_styled(&self.gui.atlas, &Text::literal(text), x, y, color, shadow);
    }
    /// The player, in a world.
    pub(super) fn native_player(&self) -> Option<NativePlayer> {
        let p = self.player.as_ref()?;
        Some(NativePlayer {
            x: p.position.x,
            y: p.position.y,
            z: p.position.z,
            yaw: self.yaw,
            pitch: self.pitch,
            dimension: self.dim.id(),
        })
    }
    /// What the crosshair is on within the player's reach: an entity in
    /// front of any block, else a block; none in the air or outside a world.
    pub(super) fn native_target(&self) -> Option<NativeTarget> {
        self.player.as_ref()?;
        if let Some(id) = self.pick_entity_hit() {
            let mob = id
                .and_then(|id| u32::try_from(id).ok())
                .and_then(|id| self.mobs.iter().find(|m| m.id == id));
            return Some(NativeTarget::Entity {
                id,
                kind: mob.map(|m| m.kind.id.clone()),
                name: mob.and_then(|m| m.custom_name.clone()),
                health: mob.map(|m| [m.health, m.kind.max_health]),
            });
        }
        let hit = self.pick()?;
        let b = self.native_block(hit.pos)?;
        Some(NativeTarget::Block {
            pos: hit.pos,
            face: hit.face,
            block: b.block,
            properties: b.properties,
        })
    }
    /// The block at `pos` in the player's world; none outside a world or
    /// where its chunk isn't loaded. Above and below the world it is air.
    pub(super) fn native_block(&self, pos: [i32; 3]) -> Option<NativeBlock> {
        let [x, y, z] = pos;
        if self.player.is_none() || !self.world.loaded(x, z) {
            return None;
        }
        let s = self.pack.assets.registry.state(self.world.block(x, y, z))?;
        let light = self.world.light(x, y, z);
        Some(NativeBlock {
            block: s.name.clone(),
            properties: s.properties.clone().into_iter().collect(),
            block_light: light & 15,
            sky_light: light >> 4,
        })
    }
    /// The biome at `pos` in the player's world (`minecraft:plains`), as the
    /// server sent it; none outside a world or where its chunk isn't loaded.
    pub(super) fn native_biome(&self, pos: [i32; 3]) -> Option<String> {
        let [x, y, z] = pos;
        self.player.as_ref()?;
        let chunk = self.world.chunks.get(&(x >> 4, z >> 4))?;
        Some(self.res.biomes().get(chunk.biome(x, y, z)).name.clone())
    }
    /// The player's inventory, in a world.
    pub(super) fn native_inventory(&self) -> Option<NativeInventory> {
        self.player.as_ref()?;
        let slot = |s: &Option<ItemStack>| s.as_ref().map(|s| self.native_slot(s));
        let armor = self.armor.iter().rev();
        let all = self.inventory.iter().chain(armor).chain([&self.offhand]);
        Some(NativeInventory {
            slots: all.map(slot).collect(),
            selected: self.selected as u32,
        })
    }
    /// A stack with its damage out of its item's durability.
    fn native_slot(&self, s: &ItemStack) -> NativeSlot {
        let items = self.res.items();
        NativeSlot {
            stack: NativeStack {
                item: s.item.to_string(),
                count: s.count,
                components: super::native_recipes::components(s),
            },
            damage: s.damage,
            max_damage: if horizon_client::stations::is_damageable(items, s) {
                items.get(&s.item).map_or(0, |i| i.max_damage)
            } else {
                0
            },
        }
    }
    /// The open screen, in a world: its name, the server's menu and title,
    /// and the menu's slots when it is one Horizon's menu engine models.
    pub(super) fn native_screen(&mut self) -> Option<NativeScreen> {
        self.player.as_ref()?;
        let screen = self.native_screen_name();
        let mut stacks = Vec::new();
        self.menu_session(|m, _, _| {
            stacks = (0..m.slot_count()).map(|i| m.slot(i).cloned()).collect();
            Some(false)
        });
        let slots = stacks
            .iter()
            .map(|s| s.as_ref().map(|s| self.native_slot(s)))
            .collect();
        let menu = self.remote_menu_name().map(str::to_owned);
        let title = self.remote.as_ref().and_then(|r| r.menu_title.as_ref());
        Some(NativeScreen {
            screen,
            title: menu.as_ref().and(title).map(Text::plain),
            menu,
            slots,
        })
    }
    /// Horizon's name for the open screen, as `native_screen` gives it.
    fn native_screen_name(&self) -> &'static str {
        match self.screen {
            Screen::Playing => "playing",
            Screen::GameModeSwitcher => "game_mode_switcher",
            Screen::Paused => "paused",
            Screen::Options => "options",
            Screen::Chat => "chat",
            Screen::Creative => "creative",
            Screen::Inventory => "inventory",
            Screen::Dead => "dead",
            Screen::Stats => "stats",
            Screen::Advancements => "advancements",
            Screen::WorldOptions => "world_options",
            Screen::Friends => "friends",
            Screen::CommandBlock => "command_block",
            Screen::ServerLinks => "server_links",
            Screen::ConfirmLink => "confirm_link",
            Screen::DialogWarning => "dialog_warning",
            Screen::Win => "win",
            Screen::Mod => "mod",
        }
    }
    /// The scoreboard sidebar the HUD shows, in a world; none when it shows
    /// none.
    pub(super) fn native_sidebar(&self) -> Option<NativeSidebar> {
        self.player.as_ref()?;
        let objective = self.sidebar_objective()?;
        let lines = hud_overlays::sidebar_rows(&self.score.board, objective)
            .iter()
            .map(|(name, score)| (name.plain(), score.plain()))
            .collect();
        Some(NativeSidebar {
            title: objective.display_text().resolve_legacy().plain(),
            lines,
        })
    }
    /// The boss bars the HUD shows, top first, in a world.
    pub(super) fn native_boss_bars(&self) -> Vec<NativeBossBar> {
        if self.player.is_none() {
            return Vec::new();
        }
        let now = horizon_client::boss_overlay::now_millis();
        self.boss_overlay
            .bars(now)
            .iter()
            .map(|b| NativeBossBar {
                name: b.name.plain(),
                progress: b.progress,
                color: match b.color {
                    0 => "pink",
                    1 => "blue",
                    2 => "red",
                    3 => "green",
                    4 => "yellow",
                    5 => "purple",
                    _ => "white",
                },
                notches: match b.overlay {
                    1 => 6,
                    2 => 10,
                    3 => 12,
                    4 => 20,
                    _ => 0,
                },
            })
            .collect()
    }
    /// The world the player is in, in a world: single player or a server.
    pub(super) fn native_world(&self) -> Option<NativeWorld> {
        self.player.as_ref()?;
        Some(self.native_world_now())
    }
    /// The world as `native_world` reads it, for `native_join` before the
    /// player is placed.
    pub(super) fn native_world_now(&self) -> NativeWorld {
        let single_player = self.has_singleplayer_server();
        let name = match &self.integrated {
            Some(server) => server.world_name.clone(),
            None => self.level.name.clone(),
        };
        let address = self
            .remote
            .as_ref()
            .map(|r| horizon_client::net::shown_address(&r.session.address));
        NativeWorld {
            single_player,
            published: single_player && self.is_published(),
            name: single_player.then_some(name),
            address: address.filter(|_| !single_player),
        }
    }
    /// The effects on the player, in a world; none when it has none.
    pub(super) fn native_effects(&self) -> Vec<NativeEffect> {
        if self.player.is_none() {
            return Vec::new();
        }
        let list = self.potions.effects.list.iter();
        list.map(|e| NativeEffect {
            id: ["minecraft:", e.id].concat(),
            amplifier: e.amplifier,
            duration: e.duration,
            ambient: e.ambient,
            particles: e.visible,
        })
        .collect()
    }
    /// The player's health, food, air and experience, in a world.
    pub(super) fn native_vitals(&mut self) -> Option<NativeVitals> {
        let p = self.player.as_mut()?;
        Some(NativeVitals {
            health: p.health.health(),
            max_health: p.attribute("minecraft:max_health").unwrap_or(20.0) as f32,
            absorption: p.health.absorption(),
            armor: p
                .attribute("minecraft:armor")
                .or_else(|_| p.attribute("armor"))
                .unwrap_or(0.0) as i32,
            food: p.health.food.food,
            saturation: p.health.food.saturation,
            air: p.air,
            xp_level: self.experience.level,
            xp_progress: self.experience.progress,
            xp_total: self.experience.total,
            mode: self.mode.name(),
            hardcore: self.level.hardcore,
        })
    }
    /// The living mobs and other players within `radius` blocks of the
    /// player's feet, nearest first; none outside a world.
    // shortcut: mobs and players only (no items, vehicles, frames), add them when a mod needs them.
    pub(super) fn native_entities(&self, radius: f64) -> Vec<NativeEntity> {
        let Some(p) = self.player.as_ref() else {
            return Vec::new();
        };
        let at = [p.position.x, p.position.y, p.position.z];
        let distance = |e: &NativeEntity| (0..3).map(|a| (e.pos[a] - at[a]).powi(2)).sum::<f64>();
        let mut list: Vec<NativeEntity> = self
            .mobs
            .iter()
            .filter(|m| m.alive())
            .map(|m| NativeEntity {
                id: m.id,
                kind: m.kind.id.to_string(),
                pos: [m.position.x, m.position.y, m.position.z],
                name: self
                    .remote_player_name(m.id)
                    .map(str::to_owned)
                    .or_else(|| m.custom_name.clone()),
                health: [m.health, m.kind.max_health],
            })
            .filter(|e| distance(e) <= radius * radius)
            .collect();
        list.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        list
    }
    /// The player list, the player included, in the server's order; none
    /// outside a world.
    pub(super) fn native_players(&mut self) -> Vec<NativeListPlayer> {
        if self.player.is_none() {
            return Vec::new();
        }
        let players = self.tab_players().0;
        let board = &self.score.board;
        let entry = |p: TabPlayer| NativeListPlayer {
            uuid: p.uuid,
            display: tab_list::name_for_display(&p, board).plain(),
            mode: super::game_mode::mode_from_id(p.game_mode).name(),
            latency: p.latency,
            name: p.name,
        };
        players.into_iter().map(entry).collect()
    }
    /// The world's clock and weather, in a world.
    pub(super) fn native_time(&self) -> Option<NativeTime> {
        self.player.as_ref()?;
        let day_time = self.world.day_time;
        let (rain, thunder) = self.world.weather.levels(1.0);
        Some(NativeTime {
            game_time: self.world.game_time,
            day: day_time.div_euclid(24000),
            time_of_day: day_time.rem_euclid(24000),
            rain,
            thunder,
        })
    }
    /// The frame counter: frames in the last second, the slowest frame and
    /// the 1% low of the last 5 seconds, in frames per second.
    pub(super) fn native_frame_rate(&self) -> (u32, u32, u32) {
        self.frame_reading()
    }
    /// Attack and use clicks in the last second.
    pub(super) fn native_clicks(&mut self) -> (u32, u32) {
        self.mouse_reading().0
    }
    /// A vanilla mapping (`key.forward`, `key.jump`, `key.attack`…): whether
    /// it is held, and its key's label as Controls shows it.
    pub(super) fn native_mapping(&mut self, name: &str) -> Option<(bool, String)> {
        let &(_, key) = MAPPINGS.iter().find(|m| m.0 == name)?;
        let label = keybinds::display_name(self.settings.keys.get(name)).plain();
        let down = match key {
            Some(k) => self.keys.contains(&k),
            None => self.mouse_reading().1[usize::from(name == "key.use")],
        };
        Some((down, label))
    }
    /// The key the player chose for a mod's key `name`, else `default`
    /// (`key.keyboard.z`), as the WebAssembly mods' keys were kept
    /// (`key.<mod id>.<name>` in `horizon-mods.txt`).
    pub(super) fn native_mod_key(&self, mod_id: &str, name: &str, default: &str) -> String {
        let chosen = self
            .settings
            .mods
            .keys
            .get(mod_id)
            .and_then(|k| k.get(name));
        chosen.map_or(default, String::as_str).to_owned()
    }
    /// A line in chat from the client.
    pub(super) fn native_chat(&mut self, text: &str) {
        let focused = self.screen == Screen::Chat;
        let line = Text::literal(text);
        let atlas = &self.gui.atlas;
        self.ui
            .chat
            .add(atlas, line, None, ChatSource::Client, self.ticks, focused);
    }
    /// A native mod's own folder: `horizon-mods/.horizon-mod-data/<id>/`,
    /// where the WebAssembly mod of the same id kept its storage, so a
    /// player's settings carry over. Not created here.
    pub(super) fn native_data_dir(&self, mod_id: &str) -> PathBuf {
        let mods = Self::options_path().with_file_name(mod_list::DIR);
        mods.join(".horizon-mod-data").join(mod_id)
    }
    /// A mod's setting `key` (one file per key in `native_data_dir`).
    pub(super) fn native_setting(&self, mod_id: &str, key: &str) -> Option<String> {
        let path = self.native_data_dir(mod_id).join(key);
        fs::read_to_string(path).ok().map(|v| v.trim().to_owned())
    }
    /// Keeps a mod's setting `key`.
    pub(super) fn native_set_setting(&self, mod_id: &str, key: &str, value: &str) {
        let dir = self.native_data_dir(mod_id);
        let _ = fs::create_dir_all(&dir).and_then(|()| fs::write(dir.join(key), value));
    }
    /// Shows the panel beside container screens with these items (at most
    /// 8192), as the game's own slots in pages, with a search box; the
    /// player's clicks, searches and keys over items come back as
    /// `native_panel_event`. The panel is the last mod's to show one; empty
    /// items and no recipe page take yours away.
    pub(super) fn native_panel_items(&mut self, mod_id: &str, items: Vec<NativeStack>) {
        self.panel.show_items(mod_id, items);
    }
    /// Shows these recipes in the panel instead of its items, as many as
    /// fit a page, under `title` and a back button, with a row of `tabs`
    /// (each an icon; `tab` is the selected one) between them. At most 256
    /// recipes and 16 tabs; no recipes shows the items again. The same title
    /// and tab again keep the page the player turned to.
    pub(super) fn native_panel_recipes(
        &mut self,
        mod_id: &str,
        title: &str,
        tabs: Vec<NativeStack>,
        tab: u32,
        views: Vec<NativeRecipeView>,
    ) {
        let page = RecipePage::new(title, tabs, tab, views);
        self.panel.show_recipes(mod_id, page);
    }
    /// Every item and recipe the game knows, made once (the same for every
    /// call); none outside a world. They are the window's own: the world's
    /// in single player, vanilla's on a server, which sends a client no
    /// others.
    pub(super) fn native_recipes(&mut self) -> Option<Arc<NativeRecipes>> {
        self.player.as_ref()?;
        self.made_recipes()
    }
    /// The `book_id`s of the recipes the player's recipe book holds now
    /// (the server's book on a server); empty outside a world.
    pub(super) fn native_known_recipes(&mut self) -> Arc<[i32]> {
        if self.player.is_none() {
            return Arc::from([]);
        }
        self.known_displays()
    }
    /// Places a recipe in the open screen's grid as a click in the recipe
    /// book does (`ServerboundPlaceRecipePacket`): the inventory's grid or a
    /// crafting table's. `all` places as many as the items allow, as with
    /// Shift. Refused while no such screen is open and for a recipe the
    /// player's book doesn't hold; one missing items shows as a ghost.
    pub(super) fn native_place_recipe(&mut self, book_id: i32, all: bool) -> Result<(), String> {
        self.recipe_place(book_id, all)
    }
    /// Creative mode only: gives the player `count` of `item`
    /// (`minecraft:diamond`) as taking it from the creative inventory does
    /// (`ServerboundSetCreativeModeSlot`): the selected slot first, then the
    /// hotbar, then the inventory. The count is cut to one stack.
    pub(super) fn native_give(&mut self, item: &str, count: u32) -> Result<(), String> {
        self.recipe_give(item, count)
    }
    /// Scales the volume of sound event `id` (`minecraft:entity.ghast.scream`)
    /// by `factor` from its next play on (0 mutes); none plays it as Horizon
    /// does again.
    pub(super) fn native_sound_volume(&mut self, id: &str, factor: Option<f32>) {
        match factor {
            Some(factor) => self.audio.scales.insert(id.to_owned(), factor),
            None => self.audio.scales.remove(id),
        };
    }
}

/// Where the game calls the hook points.
impl Game<'_> {
    /// Options > Mods' rows for the native mods, after the others.
    pub(super) fn list_native_mods(&mut self) {
        let mut mods = Vec::new();
        self.native_mods(&mut mods);
        // A WebAssembly mod of the same id doesn't run (`start_mods`): the
        // native row stands for it, and alone toggles its `mod.<id>` line.
        self.settings
            .mods
            .installed
            .retain(|row| row.native.is_none() && !mods.iter().any(|m| m.id == row.id));
        for m in mods {
            let mut buttons = Vec::new();
            self.native_settings(&m.id, &mut buttons);
            let page = NativePage {
                keys: m.keys,
                buttons,
            };
            self.settings.mods.installed.push(Installed {
                id: m.id,
                title: m.title,
                description: m.description,
                problem: None,
                dev: false,
                has_settings: false,
                native: Some(page),
            });
        }
    }
    /// A click on button `button` of native mod `i`'s page (its index in
    /// Options > Mods), and the page's labels after it.
    pub(super) fn native_settings_event(&mut self, i: usize, button: usize) {
        let Some(id) = self.settings.mods.installed.get(i).map(|m| m.id.clone()) else {
            return;
        };
        self.native_settings_click(&id, button);
        self.native_settings_refresh(i);
    }
    /// Native mod `i`'s page opened: its labels asked again, so that they
    /// follow the language (and the mod's state) since the list was made.
    pub(super) fn native_settings_refresh(&mut self, i: usize) {
        let Some(id) = self.settings.mods.installed.get(i).map(|m| m.id.clone()) else {
            return;
        };
        let mut buttons = Vec::new();
        self.native_settings(&id, &mut buttons);
        let row = self.settings.mods.installed.get_mut(i);
        if let Some(page) = row.and_then(|m| m.native.as_mut()) {
            page.buttons = buttons;
        }
    }
    /// A key event for `native_key`.
    pub(super) fn native_key_event(&mut self, key: KeyCode, pressed: bool, repeat: bool) {
        if repeat {
            return;
        }
        if let Some(name) = keybinds::key_name(key) {
            if pressed {
                self.native_panel_key(name);
            }
            self.native_key(name, pressed);
        }
    }
    /// A mouse button while playing, for `native_key`.
    pub(super) fn native_mouse_event(&mut self, button: MouseButton, pressed: bool) {
        let name = keybinds::mouse_name(button);
        self.native_key(&name, pressed);
    }
    /// Wheel steps while playing: whether a native mod took them.
    pub(super) fn native_wheel_event(&mut self, steps: f32) -> bool {
        let mut claimed = false;
        self.native_wheel(steps, &mut claimed);
        claimed
    }
    /// Once a frame: tell native mods when the screen or menu changed since
    /// the last frame in this world. Off the title screen the last one is
    /// kept while no player is placed (respawn, a new dimension).
    pub(super) fn native_screen_event(&mut self) {
        if self.title.is_some() {
            self.extras.native_screen = None;
            return;
        }
        if self.player.is_none() {
            return;
        }
        let now = (
            self.native_screen_name(),
            self.remote_menu_name().map(str::to_owned),
        );
        let before = self.extras.native_screen.replace(now.clone());
        if before.is_some_and(|b| b != now) {
            self.native_screen_changed(now.0, now.1.as_deref());
        }
    }
    /// A chat line arrived: whether a native mod hides it.
    pub(super) fn native_chat_hidden(&mut self, text: &str) -> bool {
        let mut hide = false;
        self.native_chat_received(text, &mut hide);
        hide
    }
    /// The player sends a chat line: whether a native mod cancelled it.
    pub(super) fn native_chat_cancelled(&mut self, text: &str) -> bool {
        let mut cancel = false;
        self.native_chat_send(text, &mut cancel);
        cancel
    }
    /// The lines native mods add under `stack`'s tooltip.
    pub(super) fn native_tooltip_lines(&self, stack: &ItemStack) -> Vec<(Text, u32)> {
        let mut lines = Vec::new();
        self.native_tooltip(&self.native_slot(stack), &mut lines);
        lines
            .into_iter()
            .map(|(s, c)| (Text::legacy(&s), c))
            .collect()
    }
    /// A mouse press while playing: an attack or a use, told with its target.
    pub(super) fn native_pressed(&mut self, button: MouseButton) {
        let attack = match button {
            MouseButton::Left => true,
            MouseButton::Right => false,
            _ => return,
        };
        let target = self.native_target();
        if attack {
            self.native_attack(target.as_ref());
        } else {
            self.native_use(target.as_ref());
        }
    }
    /// The player broke `state` at `pos`, before the world loses it.
    pub(super) fn native_broke(&mut self, pos: [i32; 3], state: BlockStateId) {
        if let Some(s) = self.pack.assets.registry.state(state) {
            let block = s.name.clone();
            self.native_break(pos, &block);
        }
    }
    /// Tells `native_sound` the sounds started since the last frame; the
    /// first call starts keeping them.
    pub(super) fn native_sound_event(&mut self) {
        let played = self.audio.played.replace(Vec::new());
        for (id, pos, volume) in played.unwrap_or_default() {
            self.native_sound(&id, pos, volume);
        }
    }
    /// A client mod's command: a native mod's first, then a WebAssembly mod's.
    pub(super) fn client_mod_command(&mut self, command: &str) -> bool {
        let mut ran = false;
        self.native_command(command, &mut ran);
        ran || self.mod_command(command)
    }
    /// The zoom in effect while playing (a native mod's); the spyglass's own
    /// view wins.
    pub(super) fn native_zoom_now(&self) -> Option<NativeZoom> {
        if !self.native_playing() || self.scoping() {
            return None;
        }
        let mut zoom = None;
        self.native_zoom(&mut zoom);
        // As the WebAssembly runtime clamped a mod's zoom.
        let z = zoom?;
        let factor = finite(z.factor, 1.0, 10.0);
        (factor > 1.0).then_some(NativeZoom {
            factor,
            smooth: z.smooth,
            turn: if z.turn.is_finite() {
                z.turn.clamp(0.05, 1.0)
            } else {
                1.0
            },
        })
    }
    /// The minimap a native mod shows.
    pub(super) fn native_minimap_now(&self) -> Option<NativeMinimap> {
        let mut minimap = None;
        self.native_minimap(&mut minimap);
        minimap.map(|m| NativeMinimap {
            size: finite(m.size, 16.0, 256.0),
            blocks_per_pixel: finite(m.blocks_per_pixel, 0.5, 4.0),
            ..m
        })
    }
}

/// Whether F3's debug screen, open when `debug`, covers `corner`
/// (`DebugScreenOverlay.extractRenderState`): its left and right columns
/// the top corners, and its charts the bottom corners `charts` (bottom left,
/// bottom right) says they are drawn in.
fn debug_takes(corner: Corner, debug: bool, charts: [bool; 2]) -> bool {
    debug
        && match corner {
            Corner::TopLeft | Corner::TopRight => true,
            Corner::BottomLeft => charts[0],
            Corner::BottomRight => charts[1],
        }
}

/// `v` within `lo..=hi`; `lo` when it is not a number.
fn finite(v: f32, lo: f32, hi: f32) -> f32 {
    if v.is_finite() { v.clamp(lo, hi) } else { lo }
}

#[cfg(test)]
mod tests {
    /// horizon-mixins refuses targets whose body has a `cfg` or a macro:
    /// the hook points and helpers must stay free of both. The launcher
    /// reads the API line as it is.
    #[test]
    fn hook_points_stay_hookable() {
        let source = include_str!("native_hooks.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or_default();
        assert!(code.contains("\npub(crate) const NATIVE_HOOKS_API: u32 = "));
        let lines = code.lines().filter(|l| !l.trim_start().starts_with("//"));
        for line in lines {
            assert!(!line.contains("cfg"), "a cfg in the hook points: {line}");
            for mac in ["!(", "![", "! {", "!{"] {
                assert!(!line.contains(mac), "a macro in the hook points: {line}");
            }
        }
    }

    /// Vanilla's F3 draws a column in each top corner (`extractLines`, left
    /// and right aligned), and its charts along the bottom.
    #[test]
    fn the_debug_screen_takes_both_top_corners_and_its_charts_corners() {
        use super::{Corner, debug_takes};
        for corner in Corner::ALL {
            assert!(!debug_takes(corner, false, [true, true]), "{corner:?}");
        }
        assert!(debug_takes(Corner::TopLeft, true, [false, false]));
        assert!(debug_takes(Corner::TopRight, true, [false, false]));
        assert!(!debug_takes(Corner::BottomLeft, true, [false, false]));
        assert!(!debug_takes(Corner::BottomRight, true, [false, false]));
        assert!(debug_takes(Corner::BottomLeft, true, [true, false]));
        assert!(!debug_takes(Corner::BottomRight, true, [true, false]));
        assert!(debug_takes(Corner::BottomRight, true, [false, true]));
    }
}
