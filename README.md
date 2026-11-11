# basso

A Baritone for [Horizon](https://github.com/horizon-rust/horizon), the Rust reimplementation of Minecraft Java Edition.

The goal is a full equivalent of [Baritone](https://github.com/cabaletta/baritone), written in Rust and running
inside Horizon:

- pathfinding that walks, sprints, jumps, parkours, swims, falls, pillars up, digs down and breaks through what is
  in the way
- goals: go to a position or a block, mine a block type, follow an entity
- tick-accurate movement costs and execution, so a path that is planned is a path that works

Baritone is a voice range; basso is the next one down.

## Hard fork of Azalea

basso is a hard fork of [Azalea](https://github.com/azalea-rs/azalea) (MIT), a Rust library for Minecraft Java
Edition bots whose pathfinder is already Baritone-style. Azalea connects to servers over the network; basso instead
aims to run in the game itself, on Horizon's world, physics and ticks. It does not track upstream, but fixes may be
picked from it.

## Status

The repository contains a native Horizon bundle at `horizon-mixin.toml`. Build it with Horizon's launcher to prove the
in-game bridge: `/basso` and `/basso help` are registered in chat and reply locally. The pathfinder remains an Azalea
hard fork as of `0.16.0+mc26.2`; adapting its world, physics and tick inputs to Horizon is the next step.

## Tests

Run the tests for everything you change (`cargo test -p <crate>`, and `cargo test --workspace` before calling a task
done), and write as many unit tests as possible: every new function, fix or behaviour gets tests, a fix gets one that
fails without it. Never call work done while a test fails.

## License

MIT, see [LICENSE.md](LICENSE.md). Azalea's copyright notice is kept there.
