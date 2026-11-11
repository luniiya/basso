basso is a pathfinding-focused fork of [Azalea](https://github.com/azalea-rs/azalea) for use with Minecraft bots that run in-process.

Networking, authentication, chat signing and swarm support have been removed. The client is fed world and player state by the host (Horizon), and outbound actions such as movement and mining are emitted as plain ECS messages (`SendGamePacketEvent`).

# Examples

The pathfinder can be tested without a server through the fake-world `Simulation` harness in `azalea/src/pathfinder/execute/simulation.rs`. Run the tests with:

```sh
cargo test -p azalea pathfinder
```

# Building

basso builds on stable Rust. The baseline is Rust 1.95, which is what Horizon uses.

# Plugins

Azalea uses [Bevy ECS](https://docs.rs/bevy_ecs) internally to store information about the world and clients. Bevy plugins are more powerful than async handler functions, but more difficult to use. See [pathfinder](src/pathfinder/mod.rs) as an example of how to make a plugin.

Everything related to the player and world is implemented as a Bevy plugin. See `bot::DefaultBotPlugins` for the list that is enabled by default.

Also note that just because something is an entity in the ECS doesn't mean that it's a Minecraft entity. You can filter for that by having `With<MinecraftEntityId>` as a filter.

See the [Bevy Cheatbook](https://bevy-cheatbook.github.io/programming/ecs-intro.html) to learn more about Bevy ECS (and the ECS paradigm in general).

# Debugging

## Logging

One of the most useful tools for debugging issues is logging. The default log level is `info`, but you can make it show more or less information by changing the log level. Enabling logging is done with `RUST_LOG=debug cargo run` on Linux/bash or `set RUST_LOG=debug && cargo run` on Windows. The log levels are `trace`, `debug`, `info`, `warn`, and `error`, in ascending priority.

## Deadlocks

If your code is simply hanging, it might be a deadlock. Enable `parking_lot`'s `deadlock_detection` feature and copy the deadlock block from the Azalea examples to the beginning of your code and it'll print a long backtrace if a deadlock is detected.

## Backtraces

Backtraces are also useful, though they're sometimes hard to read and don't always contain the actual location of the error. Run your code with `RUST_BACKTRACE=1` to enable full backtraces. If it's very long, often searching for the keyword "azalea" will help you filter out unrelated things and find the actual source of the issue.
