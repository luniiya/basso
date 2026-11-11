// The Horizon side of basso, injected as `crate::game::basso_adapter`: it
// shows Horizon's world, player and entities to `basso_engine` and applies
// what the engine decides. The hook helpers are one-line wrappers around it;
// everything that can be tested lives in the engine.

use std::sync::{Mutex, MutexGuard};

use basso_engine::{
    blocks::{self, BlockTraits},
    bot::{Bot, EntityInfo},
    executor::{Controls, PlayerState},
    pos::{BlockPos, Vec3},
    render,
    world::{BlockView, Cell},
};
use horizon_client::{chat::ChatSource, held::Source, text::Text};
use winit::{event::MouseButton, keyboard::KeyCode};

use super::{Game, Screen};
use horizon_client::mod_api::NativeBlockRim;

struct State {
    bot: Bot,
    /// The controls applied last tick, to let go of what is no longer held.
    held: Controls,
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
        }
    });
    let others = game.others.iter().map(|o| EntityInfo {
        id: o.id as i32,
        kind: o.kind.clone(),
        name: None,
        is_player: o.kind == "minecraft:player",
        living: false,
        pos: vec3(o.position),
    });
    mobs.chain(others).collect()
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
    chat_line(game, &format!("> {echo}"), ECHO_COLOR);
    let Some(player) = player_state(game) else {
        say(game, vec!["Not in a world.".to_owned()]);
        return true;
    };
    let lines = {
        let mut guard = state();
        let st = guard.get_or_insert_with(|| State {
            bot: Bot::new(),
            held: Controls::default(),
        });
        st.bot.command(line, &View(game), &player)
    };
    say(game, lines);
    true
}

pub(super) fn tick(game: &mut Game<'_>) {
    if !game.native_window() {
        return;
    }
    let mut guard = state();
    let Some(st) = guard.as_mut() else {
        return;
    };
    let controls = match player_state(game) {
        Some(player) if game.screen == Screen::Playing => {
            let entities = entities(game);
            st.bot.tick(&View(game), &player, &entities)
        }
        // a menu or chat is open, or the player is gone: keep the order
        _ => None,
    };
    let messages = st.bot.take_messages();
    let controls = controls.unwrap_or_default();
    apply(game, &st.held, &controls);
    st.held = controls;
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

fn apply(game: &mut Game<'_>, was: &Controls, now: &Controls) {
    if let Some((yaw, pitch)) = now.look {
        game.yaw = yaw;
        game.pitch = pitch.clamp(-90.0, 90.0);
    }
    key(game, KeyCode::KeyW, was.forward, now.forward);
    key(game, KeyCode::ControlLeft, was.sprint, now.sprint);
    key(game, KeyCode::Space, was.jump, now.jump);
    let (mining, mine) = (was.mine.is_some(), now.mine.is_some());
    if mining != mine {
        let _ = game.mouse_mapping(MouseButton::Left, mine, Source::Agent);
    }
}

/// The path, the blocks to break and the goal, as block outlines.
pub(super) fn visuals(game: &Game<'_>, rims: &mut Vec<NativeBlockRim>) {
    let guard = state();
    let Some(st) = guard.as_ref() else {
        return;
    };
    if !st.bot.is_active() {
        return;
    }
    let Some(player) = player_state(game) else {
        return;
    };
    let view = View(game);
    let path = st.bot.path_nodes();
    let to_break = st.bot.blocks_to_break(&view);
    for rim in render::rims(&path, &to_break, st.bot.goal(), player.node()) {
        rims.push(NativeBlockRim {
            pos: [rim.pos.x, rim.pos.y, rim.pos.z],
            argb: rim.argb,
            width: rim.width,
        });
    }
}
