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
    executor::{Controls, PlayerState},
    pos::{BlockPos, Vec3},
    render,
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
}

impl State {
    fn new() -> Self {
        Self {
            bot: Bot::new(),
            held: Controls::default(),
            cam: None,
            keys: FlyKeys::default(),
            camera_before: None,
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

fn player_state(game: &Game<'_>) -> Option<PlayerState> {
    let p = game.player.as_ref()?;
    Some(PlayerState {
        pos: vec3(p.position),
        velocity: vec3(p.velocity),
        on_ground: p.ground.on_ground,
        in_water: p.fluids.was_touching_water,
        eye_in_water: p.fluids.was_eye_in_water,
        air: p.air,
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
    let Some(player) = player_state(game) else {
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
    let controls = match player_state(game) {
        Some(player) => {
            let entities = entities(game);
            st.bot.tick(&View(game), &player, &entities)
        }
        // the player is gone (dead, between worlds): keep the order
        None => None,
    };
    let messages = st.bot.take_messages();
    let controls = controls.unwrap_or_default();
    apply(game, &st.held, &controls);
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
    let (mining, mine) = (was.mine.is_some(), now.mine.is_some());
    if mining != mine {
        let _ = game.mouse_mapping(MouseButton::Left, mine, Source::Agent);
    }
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
        }
        (true, Some(cam)) => cam.fly(st.keys),
        (false, Some(_)) => {
            st.cam = None;
            if let Some(before) = st.camera_before.take() {
                game.camera_type = before;
            }
        }
        (false, None) => {}
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
    let _ = game;
    let mut guard = state();
    let Some(st) = guard.as_mut().filter(|st| st.driving()) else {
        return;
    };
    st.keys = FlyKeys {
        forward: *forward,
        strafe: *strafe,
        up: *jump,
        down: *sneak,
    };
    *forward = if st.held.forward { 1.0 } else { 0.0 };
    *strafe = 0.0;
    *jump = st.held.jump;
    *sneak = false;
}

// Clicks in the world are kept from the player while basso plays; menus
// and chat still take them.
pub(super) fn mouse_button(game: &Game<'_>, canceled: &mut bool) {
    if game.screen == Screen::Playing && state().as_ref().is_some_and(State::driving) {
        *canceled = true;
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
    let Some(st) = guard.as_ref().filter(|st| st.bot.is_active()) else {
        return;
    };
    let Some(player) = player_state(game) else {
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
