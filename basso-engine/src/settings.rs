//! basso's settings, after Baritone's `Settings`: named switches the player
//! sets with `/basso set <name> <value>` or flips with `/basso toggle <name>`.

/// The settings, under their Baritone names where Baritone has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Baritone's `mineScanDroppedItems`: pick up what mining drops.
    pub mine_scan_dropped_items: bool,
    /// Baritone's `autoTool`: hold the best hotbar tool for each block.
    pub auto_tool: bool,
    /// A free camera that flies with the walking keys while basso plays.
    pub freecam: bool,
    /// Baritone's `exploreForBlocks`: walk about until a wanted block shows.
    pub explore_for_blocks: bool,
    /// Baritone's `allowParkour`: sprint jumps over gaps.
    pub allow_parkour: bool,
    /// Baritone's `allowPlace`: bridge and pillar with blocks from the hotbar.
    pub allow_place: bool,
    /// Baritone's `allowBreak`: dig through blocks in the way.
    pub allow_break: bool,
    /// Clutch a long fall with a water bucket from the hotbar.
    pub mlg_water_bucket: bool,
    /// How much basso says: 0 normal, 1 verbose (paths, searches), 2 debug
    /// (every search slice, failures, targets), 3 trace.
    pub verbose: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mine_scan_dropped_items: true,
            auto_tool: true,
            freecam: true,
            explore_for_blocks: true,
            allow_parkour: true,
            allow_place: true,
            allow_break: true,
            mlg_water_bucket: true,
            verbose: 0,
        }
    }
}

/// Every setting's name, for help and completion.
pub const NAMES: &[&str] = &[
    "mineScanDroppedItems",
    "autoTool",
    "freecam",
    "exploreForBlocks",
    "allowParkour",
    "allowPlace",
    "allowBreak",
    "mlgWaterBucket",
    "verbose",
];

/// The descriptions shown by `/basso set`, in [`NAMES`] order.
const DESCRIPTIONS: &[&str] = &[
    "Pick up the items mining drops",
    "Switch to the best hotbar tool before breaking a block",
    "Watch from a camera behind the player while basso plays",
    "Explore when no wanted block is loaded nearby",
    "Jump over gaps while pathing",
    "Place blocks from the hotbar to bridge and pillar",
    "Break blocks in the way",
    "Land long falls in water from a bucket in the hotbar",
    "How much basso says: 0 normal, 1 verbose, 2 debug, 3 trace",
];

impl Settings {
    fn slot(&mut self, name: &str) -> Option<(&'static str, &mut bool)> {
        let name = NAMES.iter().filter(|n| **n != "verbose").find(|n| n.eq_ignore_ascii_case(name))?;
        let value = match *name {
            "mineScanDroppedItems" => &mut self.mine_scan_dropped_items,
            "autoTool" => &mut self.auto_tool,
            "freecam" => &mut self.freecam,
            "exploreForBlocks" => &mut self.explore_for_blocks,
            "allowParkour" => &mut self.allow_parkour,
            "allowPlace" => &mut self.allow_place,
            "allowBreak" => &mut self.allow_break,
            "mlgWaterBucket" => &mut self.mlg_water_bucket,
            _ => unreachable!("every name has a field"),
        };
        Some((name, value))
    }

    /// The value of the setting `name` (any case).
    pub fn get(&self, name: &str) -> Option<bool> {
        let mut copy = self.clone();
        copy.slot(name).map(|(_, v)| *v)
    }

    /// `/basso set [name] [value]`: list the settings, show one, or set one.
    pub fn set(&mut self, name: Option<&str>, value: Option<&str>) -> Vec<String> {
        let Some(name) = name else {
            return self.list();
        };
        if name.eq_ignore_ascii_case("verbose") {
            return self.set_verbose(value);
        }
        let Some((name, slot)) = self.slot(name) else {
            return vec![unknown(name)];
        };
        let Some(value) = value else {
            return vec![format!("{name} is {}.", on_off(*slot))];
        };
        match parse_bool(value) {
            Some(v) => {
                *slot = v;
                vec![format!("{name} is now {}.", on_off(v))]
            }
            None => vec![format!("`{value}` isn't true or false.")],
        }
    }

    fn set_verbose(&mut self, value: Option<&str>) -> Vec<String> {
        let Some(value) = value else {
            return vec![format!("verbose is {} ({}).", self.verbose, verbose_name(self.verbose))];
        };
        match parse_verbose(value) {
            Some(level) => {
                self.verbose = level;
                vec![format!("verbose is now {level} ({}).", verbose_name(level))]
            }
            None => vec![format!("`{value}` isn't a verbose level: 0-3 or normal, verbose, debug, trace.")],
        }
    }

    /// `/basso toggle <name>`.
    pub fn toggle(&mut self, name: &str) -> Vec<String> {
        if name.eq_ignore_ascii_case("verbose") {
            return vec!["verbose is a level: `basso set verbose 0-3`.".to_owned()];
        }
        let Some((name, slot)) = self.slot(name) else {
            return vec![unknown(name)];
        };
        *slot = !*slot;
        vec![format!("{name} is now {}.", on_off(*slot))]
    }

    fn list(&self) -> Vec<String> {
        let mut lines = vec!["basso settings:".to_owned()];
        for (name, description) in NAMES.iter().zip(DESCRIPTIONS) {
            let value = if *name == "verbose" {
                format!("{} ({})", self.verbose, verbose_name(self.verbose))
            } else {
                self.get(name).unwrap_or_default().to_string()
            };
            lines.push(format!("  {name} = {value} - {description}"));
        }
        lines.push("Change one with `basso set <name> <true|false>`.".to_owned());
        lines
    }
}

fn verbose_name(level: u8) -> &'static str {
    match level {
        0 => "normal",
        1 => "verbose",
        2 => "debug",
        _ => "trace",
    }
}

fn parse_verbose(value: &str) -> Option<u8> {
    match value.to_ascii_lowercase().as_str() {
        "0" | "normal" | "off" => Some(0),
        "1" | "verbose" | "on" => Some(1),
        "2" | "debug" => Some(2),
        "3" | "trace" => Some(3),
        _ => None,
    }
}

fn unknown(name: &str) -> String {
    format!("Unknown setting `{name}`. Try `basso set`.")
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_on() {
        let s = Settings::default();
        assert!(s.mine_scan_dropped_items && s.auto_tool && s.freecam && s.explore_for_blocks);
        for name in NAMES.iter().filter(|n| **n != "verbose") {
            assert_eq!(s.get(name), Some(true), "{name}");
        }
    }

    #[test]
    fn names_ignore_case() {
        let s = Settings::default();
        assert_eq!(s.get("AUTOTOOL"), Some(true));
        assert_eq!(s.get("mineScanDroppedItems"), Some(true));
        assert_eq!(s.get("nope"), None);
    }

    #[test]
    fn set_and_show() {
        let mut s = Settings::default();
        assert_eq!(s.set(Some("autotool"), Some("false")), ["autoTool is now off."]);
        assert!(!s.auto_tool);
        assert_eq!(s.set(Some("autoTool"), None), ["autoTool is off."]);
        assert_eq!(s.set(Some("autoTool"), Some("on")), ["autoTool is now on."]);
        assert!(s.auto_tool);
        assert!(s.set(Some("autoTool"), Some("maybe"))[0].contains("isn't true or false"));
        assert!(s.set(Some("nope"), Some("true"))[0].starts_with("Unknown setting"));
    }

    #[test]
    fn toggle_flips() {
        let mut s = Settings::default();
        assert_eq!(s.toggle("freecam"), ["freecam is now off."]);
        assert!(!s.freecam);
        assert_eq!(s.toggle("FreeCam"), ["freecam is now on."]);
        assert!(s.toggle("nope")[0].starts_with("Unknown setting"));
    }

    #[test]
    fn listing_shows_every_setting() {
        let lines = Settings::default().set(None, None);
        assert_eq!(lines.len(), NAMES.len() + 2);
        for name in NAMES {
            assert!(lines.iter().any(|l| l.contains(name)), "{name}");
        }
    }

    #[test]
    fn booleans() {
        for yes in ["true", "ON", "yes", "1"] {
            assert_eq!(parse_bool(yes), Some(true));
        }
        for no in ["false", "Off", "no", "0"] {
            assert_eq!(parse_bool(no), Some(false));
        }
        assert_eq!(parse_bool("2"), None);
    }

    #[test]
    fn verbose_levels_by_number_or_word() {
        let mut s = Settings::default();
        assert_eq!(s.verbose, 0);
        assert_eq!(s.set(Some("verbose"), None), ["verbose is 0 (normal)."]);
        assert_eq!(s.set(Some("verbose"), Some("2")), ["verbose is now 2 (debug)."]);
        assert_eq!(s.verbose, 2);
        s.set(Some("Verbose"), Some("trace"));
        assert_eq!(s.verbose, 3);
        s.set(Some("verbose"), Some("normal"));
        assert_eq!(s.verbose, 0);
    }

    #[test]
    fn a_bad_verbose_level_changes_nothing() {
        let mut s = Settings::default();
        s.set(Some("verbose"), Some("1"));
        let out = s.set(Some("verbose"), Some("9"));
        assert!(out[0].contains("isn't a verbose level"));
        assert_eq!(s.verbose, 1);
        assert!(s.toggle("verbose")[0].contains("level"));
    }
}
