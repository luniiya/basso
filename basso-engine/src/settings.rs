//! basso's settings, after Baritone's `Settings`: named switches the player
//! sets with `/basso set <name> <value>` or flips with `/basso toggle <name>`.

/// The settings, under their Baritone names where Baritone has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Baritone's `mineScanDroppedItems`: pick up what mining drops.
    pub mine_scan_dropped_items: bool,
    /// Baritone's `autoTool`: hold the best hotbar tool for each block.
    pub auto_tool: bool,
    /// Watch basso from a camera behind the player while it plays.
    pub freecam: bool,
    /// Baritone's `exploreForBlocks`: walk about until a wanted block shows.
    pub explore_for_blocks: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mine_scan_dropped_items: true,
            auto_tool: true,
            freecam: true,
            explore_for_blocks: true,
        }
    }
}

/// Every setting's name, for help and completion.
pub const NAMES: &[&str] = &["mineScanDroppedItems", "autoTool", "freecam", "exploreForBlocks"];

/// The descriptions shown by `/basso set`, in [`NAMES`] order.
const DESCRIPTIONS: &[&str] = &[
    "Pick up the items mining drops",
    "Switch to the best hotbar tool before breaking a block",
    "Watch from a camera behind the player while basso plays",
    "Explore when no wanted block is loaded nearby",
];

impl Settings {
    fn slot(&mut self, name: &str) -> Option<(&'static str, &mut bool)> {
        let name = NAMES.iter().find(|n| n.eq_ignore_ascii_case(name))?;
        let value = match *name {
            "mineScanDroppedItems" => &mut self.mine_scan_dropped_items,
            "autoTool" => &mut self.auto_tool,
            "freecam" => &mut self.freecam,
            "exploreForBlocks" => &mut self.explore_for_blocks,
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

    /// `/basso toggle <name>`.
    pub fn toggle(&mut self, name: &str) -> Vec<String> {
        let Some((name, slot)) = self.slot(name) else {
            return vec![unknown(name)];
        };
        *slot = !*slot;
        vec![format!("{name} is now {}.", on_off(*slot))]
    }

    fn list(&self) -> Vec<String> {
        let mut lines = vec!["basso settings:".to_owned()];
        for (name, description) in NAMES.iter().zip(DESCRIPTIONS) {
            let value = self.get(name).unwrap_or_default();
            lines.push(format!("  {name} = {value} - {description}"));
        }
        lines.push("Change one with `basso set <name> <true|false>`.".to_owned());
        lines
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
        for name in NAMES {
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
}
