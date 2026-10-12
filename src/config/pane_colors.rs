use serde::{de::Error, Deserialize, Deserializer};

/// Client presentation preferences; these do not change the server or child programs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PaneColorsConfig {
    /// Assign one of six dark color schemes using the stable pane ID.
    pub automatic: bool,
    /// Exact pane ID, manual label, or live agent name overrides, in that priority order.
    pub rules: Vec<PaneColorRuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaneColorRuleConfig {
    pub pane: String,
    #[serde(default, deserialize_with = "deserialize_theme")]
    pub theme: Option<String>,
    #[serde(default, deserialize_with = "deserialize_color")]
    pub foreground: Option<String>,
    #[serde(default, deserialize_with = "deserialize_color")]
    pub background: Option<String>,
    #[serde(default, deserialize_with = "deserialize_color")]
    pub border: Option<String>,
}

fn deserialize_theme<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    let name = Option::<String>::deserialize(de)?;
    if name.as_deref().is_some_and(|name| super::canonical_theme_name(name).is_none()) {
        return Err(D::Error::custom("unknown pane theme"));
    }
    Ok(name)
}

fn deserialize_color<'de, D: Deserializer<'de>>(de: D) -> Result<Option<String>, D::Error> {
    let value = Option::<String>::deserialize(de)?;
    if value.as_deref().is_some_and(|value| {
        let bytes = value.as_bytes();
        bytes.len() != 7 || bytes[0] != b'#' || !bytes[1..].iter().all(u8::is_ascii_hexdigit)
    }) {
        return Err(D::Error::custom("pane color must be #RRGGBB"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn pane_colors_are_opt_in_and_validate_rules() {
        assert!(!Config::default().ui.pane_colors.automatic);
        assert!(Config::default().ui.pane_colors.rules.is_empty());
        let config: Config = toml::from_str(r##"
[ui.pane_colors]
automatic = true
[[ui.pane_colors.rules]]
pane = "reviewer"
theme = "dracula"
background = "#101020"
"##).unwrap();
        assert!(config.ui.pane_colors.automatic);
        assert_eq!(config.ui.pane_colors.rules[0].pane, "reviewer");
        for value in ["#ff", "#12345z", "#üüü", "blue", ""] {
            let input = format!("[[ui.pane_colors.rules]]\npane = \"test\"\nbackground = {value:?}");
            assert!(toml::from_str::<Config>(&input).is_err(), "{value}");
        }
        assert!(toml::from_str::<Config>(r#"
[[ui.pane_colors.rules]]
pane = "test"
theme = "unknown"
"#).is_err());
    }
}
