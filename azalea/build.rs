use std::env;

fn main() {
    // save the optimization level, used by the pathfinder to warn if
    // optimizations are off
    println!(
        "cargo::rustc-env=OPT_LEVEL={}",
        env::var("OPT_LEVEL").unwrap()
    );
}
