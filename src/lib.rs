use std::path::PathBuf;
// Installed beside the binary or under the data home; development override wins.
pub fn script_path(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("LLM_MONITOR_SCRIPTS") {
        dirs.push(dir.into());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
    {
        dirs.push(bin.join("../share/maprisma-llm-monitor/scripts"));
    }
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    if let Some(data) = data_home {
        dirs.push(data.join("maprisma-llm-monitor/scripts"));
    }
    dirs.push("/usr/share/maprisma-llm-monitor/scripts".into());
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

pub mod i18n;
pub mod shot;
pub mod corners;
pub mod look;
