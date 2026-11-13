use maprisma_llm_monitor::i18n::{tr, language_options};
use serde_json::Value;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Open,
    Activate,
    Quit,
    Language(usize),
}
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    pub children: Vec<Item>,
    pub action: Option<Action>,
}
impl Item {
    pub fn find(&self, id: i32) -> Option<&Self> {
        if self.id == id {
            Some(self)
        } else {
            self.children.iter().find_map(|c| c.find(id))
        }
    }
    pub fn activate(&self, id: i32) -> Option<Action> {
        self.find(id).filter(|i| i.enabled).and_then(|i| i.action)
    }
}
pub fn menu() -> Item {
    let item = |id, label: &str, action| Item {
        id,
        label: label.into(),
        enabled: true,
        separator: false,
        children: vec![],
        action,
    };
    let mut root = item(0, "", None);
    root.children = vec![
        item(1, &tr("Open Maprisma LLM Monitor"), Some(Action::Open)),
        item(2, &tr("Quit tray"), Some(Action::Quit)),
    ];
    let mut languages = item(3, &tr("Language"), None);
    languages.children = language_options().into_iter().enumerate().map(|(index, (_, label))| item(10 + index as i32, &label, Some(Action::Language(index)))).collect();
    root.children.push(languages);
    root
}
/// The bar's chip: full and in the theme accent while a model is loaded, else the chip without its core.
pub fn icon_for(stats: &Value) -> &'static str {
    if stats["active"].as_bool() == Some(true) { super::ICON_ACTIVE } else { super::ICON_OFF }
}
pub fn tooltip(stats: &Value) -> String {
    if stats.get("error").is_some() {
        return tr("Waiting for the collector…");
    }
    if stats["active"].as_bool() != Some(true) {
        return tr("No local model loaded");
    }
    let number = |v: &Value| v.as_f64().filter(|v| v.is_finite()).unwrap_or(0.0);
    let mut text = format!(
        "{:.1}G{}",
        number(&stats["llm_vram"]) / 1073741824.0,
        if number(&stats["llm_spilled"]) > 268435456.0 {
            "!"
        } else {
            ""
        }
    );
    let p = &stats["primary"];
    let context = number(&p["context"]);
    if context > 0.0 {
        if let Some(used) = p["context_used"].as_f64().filter(|v| v.is_finite()) {
            text += &format!(" {}%", ((used / context).clamp(0.0, 1.0) * 100.0).round());
        }
    }
    text
}
#[cfg(test)]
mod tests {
    #[test]
    fn chip_icons_are_installed_and_styled() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons/");
        let install = include_str!("../../install.sh");
        for (name, class) in [
            (super::super::ICON_ACTIVE, "ColorScheme-Highlight"),
            (super::super::ICON_OFF, "ColorScheme-Text"),
        ] {
            let svg = std::fs::read_to_string(format!("{dir}{name}.svg")).unwrap();
            assert!(svg.contains(&format!(".{class} {{")) && svg.contains(&format!("class=\"{class}\"")), "{name}");
            let suffix = name.trim_start_matches("io.github.ubruckhaus.CosmicMaprismaLlmMonitor");
            assert!(install.contains(&format!("data/icons/$id{suffix}.svg")), "{name} not installed");
        }
    }
    #[test]
    fn tray_chip_follows_the_model() {
        use serde_json::json;
        assert_eq!(super::icon_for(&json!({"active":true})), super::super::ICON_ACTIVE);
        assert_eq!(super::icon_for(&json!({"active":false})), super::super::ICON_OFF);
        assert_eq!(super::icon_for(&serde_json::Value::Null), super::super::ICON_OFF);
    }
    use super::*;
    use serde_json::json;
    #[test]
    fn menu_actions() {
        let m = menu();
        assert_eq!(m.children.len(), 3);
        assert_eq!(m.find(1).unwrap().label, "Open Maprisma LLM Monitor");
        assert_eq!(m.activate(1), Some(Action::Open));
        assert_eq!(m.activate(2), Some(Action::Quit));
        assert_eq!(m.activate(0), None);
        assert_eq!(m.activate(99), None);
    }
    #[test]
    fn tooltip_states() {
        assert_eq!(tooltip(&json!({})), "No local model loaded");
        assert_eq!(
            tooltip(&json!({"error":"offline"})),
            "Waiting for the collector…"
        );
        let mut s = json!({"active":true,"llm_vram":2899102924.8,"primary":{"context":100,"context_used":31}});
        assert_eq!(tooltip(&s), "2.7G 31%");
        s["llm_spilled"] = json!(300000000);
        s["primary"]["context_used"] = json!(150);
        assert_eq!(tooltip(&s), "2.7G! 100%");
        s["primary"]["context"] = json!(0);
        assert_eq!(tooltip(&s), "2.7G!");
    }
}
