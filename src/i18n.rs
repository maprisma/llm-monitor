//! Bundled gettext catalogs shared by all three interfaces.
use serde_json::Value;
use std::{path::PathBuf, sync::LazyLock};
pub const LANGUAGES: [(&str, &str); 8] = [("en", "English"), ("de", "Deutsch"), ("fr", "Français"), ("es", "Español"), ("it", "Italiano"), ("pt", "Português"), ("nl", "Nederlands"), ("pl", "Polski")];
include!("translations_generated.rs");
static CATALOGS: LazyLock<Vec<Value>> = LazyLock::new(|| CATALOG_JSON.iter().map(|s| serde_json::from_str(s).unwrap()).collect());
pub fn system_language(values: [&str; 3]) -> &'static str {
    let value = values.into_iter().find(|v| !v.is_empty()).unwrap_or("en");
    let code = value.split(['_', '.', '@', '-']).next().unwrap_or("").to_lowercase();
    LANGUAGES.iter().find(|(c, _)| *c == code).map_or("en", |(c, _)| *c)
}
fn system() -> &'static str {
    let values = ["LC_ALL", "LC_MESSAGES", "LANG"].map(|k| std::env::var(k).unwrap_or_default());
    system_language([&values[0], &values[1], &values[2]])
}
fn config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"))
        .join("maprisma-llm-monitor/config.json")
}
fn read_setting() -> String {
    std::fs::read(config_path()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["language"].as_str().map(str::to_owned)).unwrap_or_else(|| "auto".into())
}
pub fn set_language(code: &str) -> std::io::Result<()> {
    if code != "auto" && !LANGUAGES.iter().any(|(c, _)| *c == code) { return Ok(()); }
    let path = config_path();
    let mut config = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(Value::is_object).unwrap_or_else(|| serde_json::json!({}));
    config["language"] = code.into();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&temporary, serde_json::to_vec_pretty(&config)?)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}
pub fn tr(key: &str) -> String {
    // Re-read to make changes in another running interface take effect too.
    let setting = read_setting();
    let code = if setting == "auto" { system() } else { &setting };
    let index = LANGUAGES.iter().position(|(c, _)| *c == code).unwrap_or(0);
    CATALOGS[index][key].as_str().unwrap_or(key).to_owned()
}
pub fn trf(key: &str, values: &[String]) -> String {
    let text = tr(key);
    let mut result = String::new();
    let mut rest = text.as_str();
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        if let Some(end) = rest.find('}') {
            if let Ok(index) = rest[1..end].parse::<usize>() {
                if let Some(value) = values.get(index) { result.push_str(value); rest = &rest[end+1..]; continue; }
            }
        }
        result.push('{'); rest = &rest[1..];
    }
    result.push_str(rest); result
}
pub fn language_options() -> Vec<(String, String)> {
    let code = system();
    let name = LANGUAGES.iter().find(|(c, _)| *c == code).unwrap().1;
    let selected = read_setting();
    std::iter::once(("auto", trf("System ({0})", &[name.into()])))
        .chain(LANGUAGES.iter().map(|(c, n)| (*c, n.to_string())))
        .map(|(c, label)| (c.into(), if selected == c { format!("✓ {label}") } else { label })).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn locale_precedence() {
        for (input, expected) in [(["de_DE.UTF-8", "fr", "pl"], "de"), (["", "fr-FR", "de"], "fr"), (["", "", "pl_PL@euro"], "pl"), (["zz", "de", "fr"], "en"), (["C", "de", "fr"], "en"), (["POSIX", "", "de"], "en"), (["", "", ""], "en")] { assert_eq!(system_language(input), expected); }
    }
}

/// Translate the helper's notification in Rust, preserving runtime names as data.
pub fn notify(title: &str, body: &str) {
    let title = match title {
        "GPU memory not fully freed" => tr("GPU memory not fully freed"),
        "GPU memory freed" => tr("GPU memory freed"),
        "Nothing to free" => tr("Nothing to free"),
        _ => title.to_string(),
    };
    let body = if body.starts_with("No llama-server,") {
        tr("No llama-server, Ollama or vLLM user service and no LM Studio models are running. Servers started by hand must be stopped where they run.")
    } else {
        body.split(". ").map(|part| {
            let part = part.trim_end_matches('.');
            if let Some(value) = part.strip_prefix("Could not stop: ") { trf("Could not stop: {0}.", &[value.into()]) }
            else if let Some(value) = part.strip_prefix("Could not check: ").and_then(|v| v.strip_suffix(" (systemctl --user did not answer)")) { trf("Could not check: {0} (systemctl --user did not answer).", &[value.into()]) }
            else if let Some(value) = part.strip_prefix("Stopped: ") { trf("Stopped: {0}.", &[value.into()]) }
            else { part.to_string() }
        }).collect::<Vec<_>>().join(" ")
    };
    let _ = std::process::Command::new("notify-send").args(["-a", &tr("Maprisma LLM Monitor"), &title, &body]).status();
}
