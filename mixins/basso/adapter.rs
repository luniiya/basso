// The Horizon side of basso, injected as `crate::game::basso_adapter`: it
// shows Horizon's world, player and entities to `basso_engine` and applies
// what the engine decides. The hook helpers are one-line wrappers around it;
// everything that can be tested lives in the engine.

use std::sync::{Mutex, MutexGuard};

use std::{ops::ControlFlow, sync::OnceLock, time::Instant};

use basso_engine::{
    blocks::{self, BlockTraits},
    bot::{Bot, EntityInfo},
    camera::{FlyKeys, FreeCam},
    completion::{self, Arg},
    executor::{Bucket, Controls, PlayerState, sprint_forward},
    pos::{BlockPos, Vec3},
    click, render,
    tools::{self, SlotSpeed},
    world::{BlockView, Cell},
};
use horizon_client::{
    chat::ChatSource,
    command::{ArgType, Dispatcher, MOD_COMMAND, Suggests, args::Resource},
    held::Source,
    text::Text,
    world_primitives::WorldPrimitives,
};
use winit::{event::MouseButton, keyboard::KeyCode};

use super::{Game, Screen, perspective::CameraType};

struct State {
    bot: Bot,
    /// The controls applied last tick, to let go of what is no longer held.
    held: Controls,
    /// The free camera while basso plays with `freecam` on.
    cam: Option<FreeCam>,
    /// The player's walking keys, which fly the free camera.
    keys: FlyKeys,
    /// The view the player had before the free camera took over.
    camera_before: Option<CameraType>,
    /// The player's view bobbing setting while the free camera is on.
    bobbing_before: Option<bool>,
    /// A click the mouse hook took, for the next tick to act on.
    pending_click: Option<click::Button>,
    /// basso was driving last tick, to catch the moment it takes over.
    was_driving: bool,
    /// Ticks the sprint has been wanted, for the double tap that starts it.
    sprint_ticks: u32,
}

impl State {
    fn new() -> Self {
        Self {
            bot: Bot::new(),
            held: Controls::default(),
            cam: None,
            keys: FlyKeys::default(),
            camera_before: None,
            bobbing_before: None,
            pending_click: None,
            was_driving: false,
            sprint_ticks: 0,
        }
    }

    /// basso drives the player now, so the player's own input is kept out.
    fn driving(&self) -> bool {
        self.bot.is_active()
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Horizon's world as the engine reads it.
struct View<'a, 'g>(&'a Game<'g>);

impl BlockView for View<'_, '_> {
    fn cell(&self, p: BlockPos) -> Option<Cell> {
        let game = self.0;
        if !game.world.chunks.contains_key(&(p.x >> 4, p.z >> 4)) {
            return None;
        }
        let facts = game.res.facts();
        let state = game.world.block(p.x, p.y, p.z);
        let f = facts.get(state);
        let fluid = match f.fluid {
            horizon_client::blocks::Fluid::None => blocks::Fluid::None,
            horizon_client::blocks::Fluid::Water => blocks::Fluid::Water,
            horizon_client::blocks::Fluid::Lava => blocks::Fluid::Lava,
        };
        Some(blocks::classify(&BlockTraits {
            name: &f.name,
            air: facts.is_air(state),
            full_collision: f.full_collision,
            legacy_solid: f.legacy_solid,
            fluid,
            waterlogged: f.waterlogged,
            hardness: f.hardness,
            requires_tool: f.requires_tool,
            floor_top: floor_top(&f.outline),
        }))
    }

    fn is_block(&self, p: BlockPos, wanted: &[String]) -> Option<bool> {
        let game = self.0;
        if !game.world.chunks.contains_key(&(p.x >> 4, p.z >> 4)) {
            return None;
        }
        let name = &game.res.facts().get(game.world.block(p.x, p.y, p.z)).name;
        Some(wanted.iter().any(|w| basso_engine::block_id_matches(w, name)))
    }

    fn min_y(&self) -> i32 {
        self.0.world.shape.min_y
    }

    fn knows_block(&self, id: &str) -> bool {
        self.0
            .res
            .facts()
            .states
            .iter()
            .any(|s| basso_engine::block_id_matches(id, &s.name))
    }
}

fn vec3(v: horizon_core::geometry::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

// Baritone's `colorBlockPathNodes`-style cyan for the block under the pointer.
const CLICK_COLOR: u32 = 0xFF00_FFFF;

// Baritone's acceptableThrowawayItems: blocks that are no loss to build with.
const THROWAWAY: &[&str] = &[
    "minecraft:cobblestone",
    "minecraft:stone",
    "minecraft:dirt",
    "minecraft:netherrack",
    "minecraft:cobbled_deepslate",
    "minecraft:deepslate",
    "minecraft:andesite",
    "minecraft:diorite",
    "minecraft:granite",
    "minecraft:tuff",
];

// The hotbar slot holding a block to build with: a throwaway block when
// there is one, else any other plain block (glowstone, planks) that is no
// loss to place.
fn block_slot(game: &Game<'_>) -> Option<usize> {
    let stack = |i: usize| game.inventory.get(i).and_then(Option::as_ref).filter(|s| s.count > 0);
    (0..9)
        .find(|&i| stack(i).is_some_and(|s| THROWAWAY.contains(&&*s.item)))
        .or_else(|| {
            let facts = game.res.facts();
            (0..9).find(|&i| {
                stack(i).is_some_and(|s| {
                    facts
                        .states
                        .iter()
                        .find(|b| basso_engine::block_id_matches(&s.item, &b.name))
                        .is_some_and(|b| blocks::is_buildable(&b.name, b.full_collision))
                })
            })
        })
}

// The hotbar slot holding `id`.
fn item_slot(game: &Game<'_>, id: &str) -> Option<usize> {
    (0..9).find(|&i| {
        game.inventory
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|stack| stack.count > 0 && &*stack.item == id)
    })
}

fn player_state(game: &Game<'_>, sneaking: bool) -> Option<PlayerState> {
    let p = game.player.as_ref()?;
    Some(PlayerState {
        pos: vec3(p.position),
        velocity: vec3(p.velocity),
        on_ground: p.ground.on_ground,
        in_water: p.fluids.was_touching_water,
        eye_in_water: p.fluids.was_eye_in_water,
        air: p.air,
        yaw: game.yaw,
        pitch: game.pitch,
        sneaking,
        has_blocks: block_slot(game).is_some(),
        fall_distance: p.fall_distance,
        water_bucket: item_slot(game, "minecraft:water_bucket").is_some(),
        empty_bucket: item_slot(game, "minecraft:bucket").is_some(),
    })
}

fn entities(game: &Game<'_>) -> Vec<EntityInfo> {
    let mobs = game.mobs.iter().filter(|m| !m.removed).map(|m| {
        let is_player = m.kind.id == "minecraft:player";
        EntityInfo {
            id: m.id as i32,
            kind: m.kind.id.clone(),
            name: m.custom_name.clone(),
            is_player,
            living: m.health > 0.0,
            pos: vec3(m.position),
            item: None,
        }
    });
    let others = game.others.iter().map(|o| EntityInfo {
        id: o.id as i32,
        kind: o.kind.clone(),
        name: None,
        is_player: o.kind == "minecraft:player",
        living: false,
        pos: vec3(o.position),
        item: None,
    });
    let items = game.dropped.iter().map(|i| EntityInfo {
        id: i.id as i32,
        kind: "minecraft:item".to_owned(),
        name: None,
        is_player: false,
        living: false,
        pos: vec3(i.position),
        item: Some(i.stack.item.to_string()),
    });
    mobs.chain(others).chain(items).collect()
}

/// The `[Basso]` tag's color, as Baritone's magenta tag is its own.
const TAG_COLOR: u32 = 0x55FFFF;
/// Messages, in gray as Baritone's.
const TEXT_COLOR: u32 = 0xAAAAAA;
/// The command being answered, echoed after `> `.
const ECHO_COLOR: u32 = 0xFFFFFF;

/// One chat line: the colored tag, then `text` in `color`.
fn chat_line(game: &mut Game<'_>, text: &str, color: u32) {
    let line = Text::colored("[Basso]", TAG_COLOR)
        .append(Text::colored(" ", color))
        .append(Text::colored(text, color));
    let focused = game.screen == Screen::Chat;
    let ticks = game.visual_ticks();
    let atlas = &game.gui.atlas;
    game.ui
        .chat
        .add(atlas, line, None, ChatSource::Client, ticks, focused);
}

fn say(game: &mut Game<'_>, lines: Vec<String>) {
    for line in lines {
        chat_line(game, &line, TEXT_COLOR);
    }
}

/// `/basso ...`: true when it was basso's.
pub(super) fn command(game: &mut Game<'_>, line: &str) -> bool {
    if line.split_whitespace().next() != Some("basso") {
        return false;
    }
    let echo = line.trim().trim_start_matches("basso").trim();
    if !echo.is_empty() {
        chat_line(game, &format!("> {echo}"), ECHO_COLOR);
    }
    let Some(player) = player_state(game, false) else {
        say(game, vec!["Not in a world.".to_owned()]);
        return true;
    };
    let lines = {
        let mut guard = state();
        let st = guard.get_or_insert_with(State::new);
        st.bot.command(line, &View(game), &player)
    };
    say(game, lines);
    true
}

// Runs with any screen open (chat, inventory, pause), so an order goes on
// while the player types.
pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    let mut guard = state();
    let Some(st) = guard.as_mut() else {
        return;
    };
    // clicks the player got past the mouse hook (a tap queued before basso
    // took over) are not basso's: drop them before basso's own are queued
    if st.driving() && game.screen == Screen::Playing {
        game.attack_clicks = 0;
        game.use_clicks = 0;
    }
    // a button the player still held when basso took over would stay down
    // (its release is kept from the game): let go of both
    if st.driving() && !st.was_driving {
        let _ = game.mouse_mapping(MouseButton::Left, false, Source::Agent);
        let _ = game.mouse_mapping(MouseButton::Right, false, Source::Agent);
    }
    st.was_driving = st.driving();
    let controls = match player_state(game, st.held.sneak) {
        Some(player) => {
            let entities = entities(game);
            st.bot.tick(&View(game), &player, &entities)
        }
        // the player is gone (dead, between worlds): keep the order
        None => None,
    };
    let mut messages = st.bot.take_messages();
    if let Some(button) = st.pending_click.take()
        && let Some(block) = pointed_block(game, st.cam.as_ref())
    {
        messages.push(st.bot.click_goto(click::goal(button, block)));
    }
    let controls = controls.unwrap_or_default();
    apply(game, &st.held, &controls);
    hand_with_screen_open(game, &st.held, &controls);
    if st.bot.settings.auto_tool
        && let Some(target) = controls.mine
    {
        hold_best_tool(game, target);
    }
    st.held = controls;
    watch_camera(game, st);
    drop(guard);
    say(game, messages);
}

// The top of the highest box that covers most of a block's floor.
fn floor_top(outline: &[[f64; 6]]) -> f32 {
    outline
        .iter()
        .filter(|b| (b[3] - b[0]) * (b[5] - b[2]) >= 0.75)
        .map(|b| b[4] as f32)
        .fold(0.0, f32::max)
}

fn key(game: &mut Game<'_>, key: KeyCode, was: bool, now: bool) {
    if now && !was {
        game.keys.press(key, Source::Agent);
    } else if was && !now {
        game.keys.release(&key, Source::Agent);
    }
}

// Walking and jumping go through `movement_input`, which works with chat
// open; sprint has no place there, so it stays a key.
fn apply(game: &mut Game<'_>, was: &Controls, now: &Controls) {
    if let Some((yaw, pitch)) = now.look {
        game.yaw = yaw;
        game.pitch = pitch.clamp(-90.0, 90.0);
    }
    key(game, KeyCode::ControlLeft, was.sprint, now.sprint);
    // right click: the block in hand goes on the face looked at
    let (placing, place) = (was.place.is_some(), now.place.is_some());
    if place && let Some(slot) = block_slot(game) {
        game.selected = slot;
    }
    // a bucket is used the same way: held, then right click
    if let Some(bucket) = now.bucket {
        let id = match bucket {
            Bucket::Water => "minecraft:water_bucket",
            Bucket::Empty => "minecraft:bucket",
        };
        if let Some(slot) = item_slot(game, id) {
            game.selected = slot;
        }
    }
    if (placing || was.use_item) != (place || now.use_item) {
        let _ = game.mouse_mapping(MouseButton::Right, place || now.use_item, Source::Agent);
    }
    let (mining, mine) = (was.mine.is_some(), now.mine.is_some());
    if mining != mine {
        let _ = game.mouse_mapping(MouseButton::Left, mine, Source::Agent);
    }
}

// The game breaks and places only while no screen is open (`tick_local_hand`
// checks for `Screen::Playing`), so with the inventory or chat up basso's
// clicks would sit unused. Run that step here with the screen looked past.
fn hand_with_screen_open(game: &mut Game<'_>, was: &Controls, now: &Controls) {
    let clicking = |c: &Controls| c.mine.is_some() || c.place.is_some() || c.use_item;
    if game.screen == Screen::Playing || !(clicking(was) || clicking(now)) {
        return;
    }
    let screen = std::mem::replace(&mut game.screen, Screen::Playing);
    let _ = game.tick_local_hand();
    game.screen = screen;
}

// Baritone's autoTool: the hotbar slot that breaks `target` fastest.
fn hold_best_tool(game: &mut Game<'_>, target: BlockPos) {
    let facts = game.res.facts();
    let f = facts.get(game.world.block(target.x, target.y, target.z));
    let items = game.res.items();
    let slots: Vec<SlotSpeed> = (0..9)
        .map(|i| {
            let held = game.inventory.get(i).and_then(Option::as_ref);
            SlotSpeed {
                speed: items.mining_speed(held, &f.name),
                harvests: !f.requires_tool || items.correct_for_drops(held, &f.name),
            }
        })
        .collect();
    game.selected = tools::best_slot(&slots, game.selected);
}

// The free camera follows the order: on while basso plays with `freecam`
// on, flown by the player's walking keys, and the player's own view back
// after.
fn watch_camera(game: &mut Game<'_>, st: &mut State) {
    let wanted = st.driving() && st.bot.settings.freecam;
    match (wanted, st.cam.as_mut()) {
        (true, None) => {
            let Some(p) = game.player.as_ref() else {
                return;
            };
            let eye = vec3(p.position);
            let eye = Vec3::new(eye.x, eye.y + f64::from(game.eye), eye.z);
            st.cam = Some(FreeCam::behind(eye, game.yaw, &View(game)));
            st.camera_before = Some(game.camera_type);
            // third person, so the player's body is drawn
            game.camera_type = CameraType::ThirdPersonBack;
            // no walking sway or hurt tilt on a camera outside the body
            st.bobbing_before = Some(game.settings.view_bobbing);
            game.settings.view_bobbing = false;
        }
        (true, Some(cam)) => cam.fly(st.keys),
        (false, Some(_)) => {
            st.cam = None;
            if let Some(before) = st.camera_before.take() {
                game.camera_type = before;
            }
            if let Some(before) = st.bobbing_before.take() {
                game.settings.view_bobbing = before;
            }
        }
        (false, None) => {}
    }
}

// Sprinting and speed effects do not widen the view of a camera outside the
// body.
pub(super) fn fov_modifier(modifier: &mut f32) {
    let guard = state();
    if guard.as_ref().is_some_and(|st| st.cam.is_some()) {
        *modifier = 1.0;
    }
}

// Where the free camera is this frame.
pub(super) fn camera_pose(
    game: &Game<'_>,
    partial: f32,
    position: &mut [f64; 3],
    yaw: &mut f32,
    pitch: &mut f32,
) {
    let _ = game;
    let guard = state();
    let Some(cam) = guard.as_ref().and_then(|st| st.cam) else {
        return;
    };
    let at = cam.at(f64::from(partial));
    *position = [at.x, at.y, at.z];
    *yaw = cam.yaw;
    *pitch = cam.pitch;
}

// While basso plays, the player's walking keys fly the free camera instead;
// basso's own walking goes in here, even with chat open.
pub(super) fn movement_input(
    game: &Game<'_>,
    forward: &mut f32,
    strafe: &mut f32,
    jump: &mut bool,
    sneak: &mut bool,
) {
    let mut guard = state();
    let Some(st) = guard.as_mut().filter(|st| st.driving()) else {
        return;
    };
    let sprinting = game.player.as_ref().is_some_and(|p| p.sprinting);
    st.keys = FlyKeys {
        forward: *forward,
        strafe: *strafe,
        up: *jump,
        down: *sneak,
    };
    // the sprint key does not start a sprint when sprint toggles, so tap
    // forward twice, as a player does
    let tap = if st.held.sprint && st.held.forward && !sprinting {
        let tick = st.sprint_ticks;
        st.sprint_ticks = tick.saturating_add(1);
        sprint_forward(tick)
    } else {
        st.sprint_ticks = 0;
        true
    };
    *forward = if st.held.forward && tap { 1.0 } else { 0.0 };
    *strafe = 0.0;
    *jump = st.held.jump;
    *sneak = st.held.sneak;
}

// Clicks in the world are kept from the player while basso plays; menus
// and chat still take them. In click mode the left and right buttons are
// basso's: they pick a block and send basso there.
pub(super) fn mouse_button(game: &Game<'_>, button: i32, pressed: bool, canceled: &mut bool) {
    if game.screen != Screen::Playing {
        return;
    }
    let mut guard = state();
    let Some(st) = guard.as_mut() else {
        return;
    };
    if st.bot.click_mode() && (button == 0 || button == 1) {
        if pressed {
            st.pending_click = Some(if button == 0 { click::Button::Left } else { click::Button::Right });
        }
        *canceled = true;
    } else if st.driving() {
        *canceled = true;
    }
}

// Where the view looks from and along: the free camera, else the player's eyes.
fn aim(game: &Game<'_>, cam: Option<&FreeCam>) -> Option<(Vec3, Vec3)> {
    if let Some(cam) = cam {
        return Some((cam.at(1.0), click::look_dir(cam.yaw, cam.pitch)));
    }
    let p = game.player.as_ref()?;
    let eye = vec3(p.position);
    let origin = Vec3::new(eye.x, eye.y + f64::from(game.eye), eye.z);
    Some((origin, click::look_dir(game.yaw, game.pitch)))
}

// The block a click would land on now.
fn pointed_block(game: &Game<'_>, cam: Option<&FreeCam>) -> Option<BlockPos> {
    let (origin, dir) = aim(game, cam)?;
    click::pick_block(&View(game), origin, dir, click::REACH)
}

// The outline of the block under the pointer, as Baritone's click screen draws.
fn click_box(block: BlockPos, primitives: &mut WorldPrimitives) {
    let (x, y, z) = (f64::from(block.x), f64::from(block.y), f64::from(block.z));
    let e = 0.002;
    let (lo, hi) = ([x - e, y - e, z - e], [x + 1.0 + e, y + 1.0 + e, z + 1.0 + e]);
    let corner = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    for a in 0..8usize {
        for bit in [1usize, 2, 4] {
            if a & bit == 0 {
                primitives.line(corner(a), corner(a | bit), CLICK_COLOR, 2.0).through_walls = true;
            }
        }
    }
}

// The wheel sets the free camera's speed instead of changing the hotbar slot.
pub(super) fn mouse_scrolling(accumulated: [i32; 2], canceled: &mut bool) {
    let mut guard = state();
    let Some(st) = guard.as_mut().filter(|st| st.driving()) else {
        return;
    };
    *canceled = true;
    if let Some(cam) = st.cam.as_mut() {
        cam.change_speed(f64::from(accumulated[1]));
    }
}

// The mouse turns the free camera, never the player basso is steering.
pub(super) fn turn_player(xo: f64, yo: f64) -> ControlFlow<()> {
    let mut guard = state();
    let Some(st) = guard.as_mut().filter(|st| st.driving()) else {
        return ControlFlow::Continue(());
    };
    if let Some(cam) = st.cam.as_mut() {
        cam.turn(xo, yo);
    }
    ControlFlow::Break(())
}

// Seconds since basso first drew, for the goal box's bob.
fn seconds() -> f64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

// Baritone's lines: the path, the search going on, the blocks to break and
// the goal, all seen through walls.
pub(super) fn world_primitives(game: &Game<'_>, primitives: &mut WorldPrimitives) {
    let guard = state();
    if let Some(st) = guard.as_ref().filter(|st| st.bot.click_mode())
        && let Some(block) = pointed_block(game, st.cam.as_ref())
    {
        click_box(block, primitives);
    }
    let Some(st) = guard.as_ref().filter(|st| st.bot.is_active()) else {
        return;
    };
    let Some(player) = player_state(game, false) else {
        return;
    };
    let view = View(game);
    let path = st.bot.path_nodes();
    let preview = st.bot.search_preview();
    let mut to_break = st.bot.blocks_to_break(&view);
    to_break.extend(st.bot.target_block());
    let min_y = game.world.shape.min_y;
    let scene = render::Scene {
        path: &path,
        searching: preview.as_ref().map(|(best, recent)| (&best[..], &recent[..])),
        to_break: &to_break,
        goal: st.bot.goal(),
        player: player.pos,
        seconds: seconds(),
        min_y,
        max_y: min_y + game.world.shape.height,
    };
    for line in render::lines(&scene) {
        let from = [line.from.x, line.from.y, line.from.z];
        let to = [line.to.x, line.to.y, line.to.z];
        primitives.line(from, to, line.argb, line.width).through_walls = true;
    }
}

// The chat tree with basso's arguments, so the chat box completes them.
// Every node also takes free text, which basso reads itself.
pub(super) fn chat_tree(mut tree: Dispatcher) -> Dispatcher {
    if let Some(root) = tree.find("basso") {
        add_nodes(&mut tree, root, completion::TREE);
    }
    tree
}

fn add_nodes(tree: &mut Dispatcher, parent: usize, nodes: &[completion::Node]) {
    for n in nodes {
        let id = match n.arg {
            Arg::Literal(word) => tree.literal(parent, word),
            arg => {
                let (ty, suggests) = arg_type(arg);
                let id = tree.argument(parent, n.name, ty);
                if let Some(suggests) = suggests {
                    tree.suggests(id, suggests);
                }
                id
            }
        };
        tree.executes(id, MOD_COMMAND);
        let rest = tree.argument(id, "args", ArgType::Greedy);
        tree.executes(rest, MOD_COMMAND);
        add_nodes(tree, id, n.children);
    }
}

fn arg_type(arg: Arg) -> (ArgType, Option<Suggests>) {
    match arg {
        Arg::BlockPos => (ArgType::BlockPos, None),
        Arg::ColumnPos => (ArgType::ColumnPos, None),
        Arg::Integer => (horizon_client::command::args::integer(i32::MIN), None),
        Arg::Block => (ArgType::BlockState, None),
        Arg::EntityType => (
            ArgType::Resource(Resource::EntityType),
            Some(Suggests::SummonableEntities),
        ),
        Arg::Player => (ArgType::GameProfile, None),
        Arg::OneOf(words) => (ArgType::Word, Some(Suggests::Fixed(words))),
        Arg::Bool => (ArgType::Bool, None),
        Arg::Literal(_) => unreachable!("literals are added as literals"),
    }
}

// The row in Options > Mods; the commands are typed in chat, so no keys.
pub(super) fn list(mods: &mut Vec<super::native_hooks::NativeMod>) {
    mods.push(super::native_hooks::NativeMod {
        id: "basso".into(),
        title: concat!("basso ", env!("CARGO_PKG_VERSION")).into(),
        description: "A pathfinder after Baritone: /basso goto, mine and follow.".into(),
        keys: Vec::new(),
    });
}
