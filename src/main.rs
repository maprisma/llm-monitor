// LLM Monitor for COSMIC: a panel applet for local LLMs. A dim chip while no
// model is loaded; while a runtime holds one, the GPU memory of local models and
// how full the context is, coloured green to red, with a fill line under the
// chip. It pulses while the GPU is generating. Left click opens the details,
// right click opens btop.
//
// The data comes from scripts/llm-monitor.py (the same collector as the GNOME
// extension and the Omarchy widget), which streams one JSON snapshot every two
// seconds. It only reads /proc, sysfs and the runtimes' local APIs; it never
// starts or stops a runtime.

use maprisma_llm_monitor::{corners, script_path, shot, i18n::{tr, trf, language_options, set_language, read_pref, write_pref}};
use cosmic::app::ApplicationExt;
mod window_mode;
mod window_size;
use std::process::Stdio;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cosmic::app::{Core, Task};
use cosmic::applet::cosmic_panel_config::PanelAnchor;
use cosmic::applet::{menu_button, padded_control};
use cosmic::iced::futures::SinkExt;
use cosmic::iced::futures::channel::mpsc;
use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::widget::{Row, container as iced_container, text as iced_text};
use cosmic::iced::{Alignment, Background, Border, Color, Length, Rectangle, Subscription, stream, window};
use cosmic::widget::rectangle_tracker::{RectangleTracker, RectangleUpdate, rectangle_tracker_subscription};
use cosmic::widget::{Id, autosize, button, column, container, divider, icon, mouse_area, row, scrollable, space, text};
use cosmic::{Element, theme};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};

const APP_ID: &str = "io.github.ubruckhaus.CosmicMaprismaLlmMonitor";
const ICON: &str = "io.github.ubruckhaus.CosmicMaprismaLlmMonitor-symbolic";
static AUTOSIZE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("llm-monitor-autosize"));
static NOTE_ID: LazyLock<Id> = LazyLock::new(|| Id::new("llm-monitor-shot-note"));
// F12 pictures go to ~/Pictures/<this>/ (the app's name, the same in every language).
const SHOT_FOLDER: &str = "Maprisma LLM Monitor";

const GIB: f64 = 1073741824.0;
const MIB: f64 = 1048576.0;
const SPILL_SHOWN: f64 = 268435456.0; // evicted model memory worth a "!" and a warning
const RESTART_DELAY: Duration = Duration::from_secs(5); // before a collector that exited starts again
const POPUP_WIDTH: f32 = 400.0;
const POPUP_MAX_HEIGHT: f32 = 760.0;

static FREEING: AtomicBool = AtomicBool::new(false);

fn main() -> cosmic::iced::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--notify") {
        if let (Some(title), Some(body)) = (args.get(2), args.get(3)) {
            maprisma_llm_monitor::i18n::notify(title, body);
        }
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--window") {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1).enable_all().build().expect("D-Bus runtime");
        let Some(_connection) = runtime.block_on(window_mode::claim()).expect("Window D-Bus connection") else {
            // A second `--window` start toggles (layout rule 5a): the open window closes.
            runtime.block_on(window_mode::close_running());
            return Ok(());
        };
        cosmic::app::run::<Monitor<true>>(cosmic::app::Settings::default()
            .size({ let (width, height) = window_start_size(); cosmic::iced::Size::new(width, height) })
            .resizable(Some(8.0)).transparent(false).is_daemon(false), ())
    } else {
        cosmic::applet::run::<Monitor<false>>(())
    }
}

// The `--window` size: the one the user left it at (layout rule 5a), else the popup's.
fn window_start_size() -> window_size::Size {
    window_size::parse(read_pref(window_size::KEY).as_ref()).unwrap_or((POPUP_WIDTH, POPUP_MAX_HEIGHT + 100.0))
}

// ------------------------------------------------------------------------- formatting

// Context level: green < 50 % ≤ yellow < 75 % ≤ orange < 90 % ≤ red.
fn level_color(fraction: f64) -> Color {
    if fraction >= 0.9 {
        Color::from_rgb8(0xe0, 0x1b, 0x24)
    } else if fraction >= 0.75 {
        Color::from_rgb8(0xff, 0x78, 0x00)
    } else if fraction >= 0.5 {
        Color::from_rgb8(0xf6, 0xd3, 0x2d)
    } else {
        Color::from_rgb8(0x33, 0xd1, 0x7a)
    }
}

fn num(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64).filter(|v| v.is_finite())
}

fn n(value: &Value, key: &str) -> f64 {
    num(value, key).unwrap_or(0.0)
}

fn s<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn gib(bytes: f64) -> String {
    format!("{:.1}", bytes / GIB)
}

fn pct(value: Option<f64>) -> String {
    value.map_or("—".into(), |v| format!("{}%", v.round()))
}

fn rate(value: Option<f64>) -> String {
    value.map_or("—".into(), |v| if v >= 100.0 { format!("{v:.0}") } else { format!("{v:.1}") })
}

fn mib(bytes: f64) -> String {
    if bytes >= GIB { format!("{} GiB", gib(bytes)) } else { format!("{} MiB", (bytes / MIB).round()) }
}

fn tokens(value: f64) -> String {
    if value >= 10000.0 {
        format!("{:.0}k", value / 1000.0)
    } else if value >= 1000.0 {
        format!("{:.1}k", value / 1000.0)
    } else {
        format!("{value}")
    }
}

fn ctx_text(value: f64) -> String {
    if value <= 0.0 {
        String::new()
    } else if value >= 1024.0 {
        trf("{0} ctx", &[format!("{}k", (value / 1024.0).round())])
    } else {
        trf("{0} ctx", &[value.to_string()])
    }
}

// Share of the main model's context in use, or None when unknown.
fn context_fraction(stats: &Value) -> Option<f64> {
    if stats.get("active").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let p = stats.get("primary")?;
    let context = num(p, "context").filter(|c| *c > 0.0)?;
    let used = num(p, "context_used")?;
    Some((used / context).clamp(0.0, 1.0))
}

fn models(runtime: &Value) -> &[Value] {
    runtime.get("models").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn runtime_state(runtime: &Value) -> String {
    if runtime.get("running").and_then(Value::as_bool) != Some(true) {
        return if runtime.get("installed").and_then(Value::as_bool) == Some(false) {
            tr("not installed")
        } else {
            tr("stopped")
        };
    }
    let all = models(runtime);
    let resident = all.iter().filter(|m| s(m, "state") != "sleeping").count();
    match resident {
        1 => tr("1 model loaded"),
        0 if !all.is_empty() => tr("model sleeping (GPU free)"),
        0 => tr("running, no model"),
        count => trf("{0} models loaded", &[count.to_string()]),
    }
}

// ------------------------------------------------------------------------- widgets

fn dim_text(theme: &cosmic::Theme) -> iced_text::Style {
    let mut color = Color::from(theme.cosmic().on_bg_color());
    color.a = 0.6;
    iced_text::Style { color: Some(color), ..Default::default() }
}

fn accent_text(theme: &cosmic::Theme) -> iced_text::Style {
    iced_text::Style { color: Some(Color::from(theme.cosmic().accent_text_color())), ..Default::default() }
}

fn dim<'a>(content: impl Into<std::borrow::Cow<'a, str>> + 'a) -> Element<'a, Message> {
    text::caption(content).class(theme::Text::Custom(dim_text)).into()
}

// Settings › Corners: small parts (tracks, fills, swatches) and round ones (dots, the status pill).
fn small() -> f32 {
    corners::now().small
}

fn pill_radius() -> f32 {
    corners::now().pill
}

fn fill(color: Color, radius: f32) -> theme::Container<'static> {
    theme::Container::custom(move |_| iced_container::Style {
        background: Some(Background::Color(color)),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    })
}

fn track(radius: f32) -> theme::Container<'static> {
    theme::Container::custom(move |theme: &cosmic::Theme| {
        let mut color = Color::from(theme.cosmic().on_bg_color());
        color.a = 0.12;
        iced_container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: radius.into(), ..Default::default() },
            ..Default::default()
        }
    })
}

fn accent_fill(radius: f32) -> theme::Container<'static> {
    theme::Container::custom(move |theme: &cosmic::Theme| iced_container::Style {
        background: Some(Background::Color(Color::from(theme.cosmic().accent_color()))),
        border: Border { radius: radius.into(), ..Default::default() },
        ..Default::default()
    })
}

fn neutral_fill(radius: f32) -> theme::Container<'static> {
    theme::Container::custom(move |theme: &cosmic::Theme| {
        let mut color = Color::from(theme.cosmic().on_bg_color());
        color.a = 0.45;
        iced_container::Style {
            background: Some(Background::Color(color)),
            border: Border { radius: radius.into(), ..Default::default() },
            ..Default::default()
        }
    })
}

// A rounded track with fills side by side, each (fraction, its style).
fn bar<'a>(segments: Vec<(f64, theme::Container<'static>)>, height: f32) -> Element<'a, Message> {
    let mut line = Row::new().width(Length::Fill).height(Length::Fixed(height));
    let mut used: u16 = 0;
    for (fraction, class) in segments {
        let part = (fraction.clamp(0.0, 1.0) * 1000.0).round() as u16;
        let part = part.min(1000 - used);
        if part == 0 {
            continue;
        }
        used += part;
        line = line.push(
            container(space::horizontal())
                .width(Length::FillPortion(part))
                .height(Length::Fill)
                .class(class),
        );
    }
    if used < 1000 {
        line = line.push(space::horizontal().width(Length::FillPortion(1000 - used)));
    }
    container(line).width(Length::Fill).class(track((height / 2.0).min(small()))).into()
}

fn level_bar<'a>(fraction: f64) -> Element<'a, Message> {
    bar(vec![(fraction, fill(level_color(fraction), small()))], 4.0)
}

// Label left, value right, a bar underneath.
fn meter<'a>(name: String, value: String, fraction: f64) -> Element<'a, Message> {
    column![
        row![dim(name), space::horizontal().width(Length::Fill), text::caption(value)].align_y(Alignment::Center),
        bar(vec![(fraction, neutral_fill(small()))], 6.0),
    ]
    .spacing(4)
    .into()
}

fn swatch<'a>(class: theme::Container<'static>) -> Element<'a, Message> {
    container(space::horizontal()).width(Length::Fixed(10.0)).height(Length::Fixed(10.0)).class(class).into()
}

// Swatch, name, amount; an optional note below and an optional level bar.
fn legend<'a>(
    swatch_class: theme::Container<'static>,
    name: String,
    amount: String,
    note: String,
    fraction: Option<f64>,
) -> Element<'a, Message> {
    let mut block = column![
        row![
            swatch(swatch_class),
            text::caption(name),
            space::horizontal().width(Length::Fill),
            text::caption(amount)
        ]
        .spacing(8)
        .align_y(Alignment::Center)
    ]
    .spacing(3);
    if !note.is_empty() {
        block = block.push(container(dim(note)).padding([0, 0, 0, 18]));
    }
    if let Some(fraction) = fraction {
        block = block.push(container(level_bar(fraction)).padding([0, 0, 0, 18]));
    }
    block.into()
}

fn section<'a>(title: String) -> Element<'a, Message> {
    container(text::caption_heading(title.to_uppercase()).class(theme::Text::Custom(dim_text)))
        .padding([8, 0, 0, 0])
        .into()
}

// ------------------------------------------------------------------------- applet

#[derive(Default)]
struct Monitor<const WINDOW_MODE: bool> {
    core: Core,
    window_mode: bool,
    popup: Option<window::Id>,
    rectangle: Rectangle,
    tracker: Option<RectangleTracker<u32>>,
    stats: Value,
    pulse_dim: bool,
    language_open: bool,
    language_error: bool,
    // F12: the note's state, whether a capture is on its way, the settings.
    shot: shot::Phase,
    capturing: bool,
    shot_key: bool,
    corners: String,
    corners_open: bool,
    setting_error: bool,
    resize: window_size::Debounce,
}

#[derive(Debug, Clone)]
enum Message {
    SaveSize,
    Stats(Value),
    TogglePopup,
    CloseRequested(window::Id),
    Rectangle(RectangleUpdate<u32>),
    FreeGpu,
    OpenBtop,
    Pulse,
    CheckClose,
    Language(String),
    ToggleLanguage,
    ShotKey(window::Id),
    Escape(window::Id),
    Captured(window::Screenshot),
    ShotSaved(Result<shot::Saved, String>),
    NoteInput(String),
    NoteDone,
    NoteSaved(Result<(), String>),
    SetShotKey(bool),
    ToggleCorners,
    Corners(String),
}

/// What Esc does (layout rule 5a): the innermost open thing first, then the window or popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscStep {
    /// Skip the screenshot's note (or close its error).
    Note,
    /// Close the open Language / Corners list.
    Menus,
    /// Close the window (`--window`) or the popup.
    Close,
    Nothing,
}

/// `note`: the screenshot card shows; `menus`: a list is open; `shown`: Esc came from the
/// window or the open popup (not the panel button).
fn esc_step(note: bool, menus: bool, shown: bool) -> EscStep {
    if !shown {
        EscStep::Nothing
    } else if note {
        EscStep::Note
    } else if menus {
        EscStep::Menus
    } else {
        EscStep::Close
    }
}

// Runs `work` on its own thread (saving must not hold up the popup) and answers with `done`.
fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    done: fn(Result<T, String>) -> Message,
) -> Task<Message> {
    let (sender, receiver) = cosmic::iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    Task::perform(receiver, move |result| {
        cosmic::Action::App(done(result.unwrap_or_else(|_| Err("the file could not be written".into()))))
    })
}

// Settings › F12 saves a screenshot (on unless switched off) and Corners, from config.json.
fn shot_key_setting() -> bool {
    read_pref("screenshot_key").and_then(|v| v.as_bool()).unwrap_or(true)
}

fn corners_setting() -> String {
    corners::normalize(read_pref("corners").as_ref().and_then(Value::as_str).unwrap_or("")).to_string()
}

impl<const WINDOW_MODE: bool> Monitor<WINDOW_MODE> {
    fn active(&self) -> bool {
        self.stats.get("active").and_then(Value::as_bool) == Some(true)
    }

    fn busy(&self) -> bool {
        self.stats.get("busy").and_then(Value::as_bool) == Some(true)
    }

    // The bar: chip, "2.7G 31%" while a model is loaded, the context line under the chip.
    fn panel_content(&self) -> Element<'_, Message> {
        let active = self.active();
        let fraction = context_fraction(&self.stats);
        let icon_size = self.core.applet.suggested_size(true).0;
        let chip = icon::from_name(ICON).size(icon_size).symbolic(true).icon();
        let line: Element<Message> = match fraction {
            Some(f) => container(bar(vec![(f.max(0.03), fill(level_color(f), 1.0))], 2.0))
                .width(Length::Fixed(icon_size as f32))
                .into(),
            None => space::vertical().height(Length::Fixed(2.0)).into(),
        };
        let chip = column![chip, line].spacing(2).align_x(Alignment::Center);
        if !active {
            return chip.into();
        }
        let spilled = n(&self.stats, "llm_spilled") > SPILL_SHOWN;
        let mut label = format!("{}G{}", gib(n(&self.stats, "llm_vram")), if spilled { "!" } else { "" });
        if let Some(f) = fraction {
            label += &format!(" {}%", (f * 100.0).round());
        }
        let mut color = fraction.map(level_color);
        if self.busy() && self.pulse_dim {
            // Pulses while the GPU is generating.
            let mut c = color.unwrap_or(Color::WHITE);
            c.a = 0.45;
            color = Some(c);
        }
        let label = self.core.applet.text(label);
        let label: Element<Message> = match color {
            Some(c) => label.class(theme::Text::Color(c)).into(),
            None => label.class(theme::Text::Custom(accent_text)).into(),
        };
        let horizontal = matches!(self.core.applet.anchor, PanelAnchor::Top | PanelAnchor::Bottom);
        if horizontal {
            row![chip, label].spacing(6).align_y(Alignment::Center).into()
        } else {
            column![chip, label].spacing(2).align_x(Alignment::Center).into()
        }
    }

    fn runtime_block<'a>(&self, runtime: &Value) -> Element<'a, Message> {
        let all = models(runtime);
        let loaded = all.iter().any(|m| s(m, "state") != "sleeping");
        let running = runtime.get("running").and_then(Value::as_bool) == Some(true);
        let dot: theme::Container<'static> = if loaded {
            accent_fill(pill_radius())
        } else if running {
            neutral_fill(pill_radius())
        } else {
            track(pill_radius())
        };
        let state = runtime_state(runtime);
        let state: Element<Message> = if loaded {
            text::caption(state).class(theme::Text::Custom(accent_text)).into()
        } else {
            dim(state)
        };
        let name = s(runtime, "name").to_string();
        let name: Element<Message> = if loaded { text::heading(name).into() } else { text::body(name).into() };
        let mut block = column![
            row![
                container(space::horizontal()).width(Length::Fixed(8.0)).height(Length::Fixed(8.0)).class(dot),
                name,
                space::horizontal().width(Length::Fill),
                state
            ]
            .spacing(8)
            .align_y(Alignment::Center)
        ]
        .spacing(3);

        for model in all {
            let context = n(model, "context");
            let used = num(model, "context_used");
            let has_usage = used.is_some() && context > 0.0;
            let mut title = s(model, "name").to_string();
            if context > 0.0 && !has_usage {
                title += &format!("  ·  {}", ctx_text(context));
            }
            let state = s(model, "state");
            if !state.is_empty() && state != "loaded" {
                let state = match state { "sleeping" => tr("sleeping"), "loading" => tr("loading"), "unloaded" => tr("unloaded"), _ => state.to_string() };
                title += &format!("  ·  {state}");
            }
            block = block.push(container(text::caption(title)).padding([0, 0, 0, 16]));
            let Some(used) = used.filter(|_| has_usage) else { continue };
            // Tokens in the context window, and the VRAM reserved for it (KV cache).
            let fraction = used / context;
            let memory = n(model, "context_memory");
            let caption = if memory > 0.0 { format!("{}  ·  {}", tr("Context"), trf("{0} VRAM", &[mib(memory)])) } else { tr("Context") };
            let value = trf("{0} / {1} tokens ({2}%)", &[tokens(used), tokens(context), (fraction * 100.0).round().to_string()]);
            let value: Element<Message> = if fraction >= 0.9 {
                text::caption(value).class(theme::Text::Color(level_color(fraction))).into()
            } else {
                text::caption(value).into()
            };
            block = block.push(
                container(row![dim(caption), space::horizontal().width(Length::Fill), value].align_y(Alignment::Center))
                    .padding([0, 0, 0, 16]),
            );
            block = block.push(container(level_bar(fraction)).padding([0, 0, 0, 16]));
        }

        if running {
            let spilled = n(runtime, "spilled");
            let mut details = vec![
                format!(
                    "{}{}",
                    trf("VRAM {0} GiB", &[gib(n(runtime, "vram"))]),
                    if spilled > SPILL_SHOWN { format!(" {}", trf("(+{0} GiB in RAM)", &[gib(spilled)])) } else { String::new() }
                ),
                format!("{} {}", tr("CPU"), pct(num(runtime, "cpu"))),
                trf("RAM {0} GiB", &[gib(n(runtime, "memory"))]),
            ];
            let clients: Vec<&str> = runtime
                .get("clients")
                .and_then(Value::as_array)
                .map(|c| c.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            if !clients.is_empty() {
                details.push(trf("used by {0}", &[clients.join(", ")]));
            }
            block = block.push(container(dim(details.join("  ·  "))).padding([0, 0, 0, 16]));
        }
        block.into()
    }

    fn details(&self) -> Element<'_, Message> {
        let stats = &self.stats;
        let null = Value::Null;
        let gpu = stats.get("gpu").unwrap_or(&null);
        let system = stats.get("system").unwrap_or(&null);
        let primary = stats.get("primary").filter(|p| p.is_object());
        let p = primary.unwrap_or(&null);
        let total = n(gpu, "vram_total");
        let free = (total - n(gpu, "vram_used")).max(0.0);
        let all = if total > 0.0 { total } else { 1.0 };

        // Header: title, what is loaded, generating / idle.
        let meta = match primary {
            Some(p) => {
                let model = s(p, "model");
                if model.is_empty() { s(p, "runtime").to_string() } else { format!("{} · {model}", s(p, "runtime")) }
            }
            None => tr("No local model loaded"),
        };
        let pill = if self.busy() { tr("generating") } else if self.active() { tr("idle") } else { String::new() };
        let mut header = row![
            icon::from_name(ICON).size(32).symbolic(true).icon(),
            column![text::title4(tr("Maprisma LLM Monitor")), dim(meta)].spacing(2),
            space::horizontal().width(Length::Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if !pill.is_empty() {
            header = header.push(container(text::caption(pill)).padding([2, 10]).class(track(pill_radius())));
        }

        let mut content = column![header, divider::horizontal::default()].spacing(8);

        // GPU memory: total, one stacked bar (LLM / desktop & apps / free), legend.
        content = content.push(section(tr("GPU memory")));
        content = content.push(
            row![
                text::heading(trf("{0} of {1} GiB in use", &[gib(n(gpu, "vram_used")), gib(total)])),
                space::horizontal().width(Length::Fill),
                dim(trf("{0} GiB free", &[gib(free)])),
            ]
            .align_y(Alignment::Center),
        );
        content = content.push(bar(
            vec![
                (n(stats, "llm_vram") / all, accent_fill(0.0)),
                (n(stats, "other_vram") / all, neutral_fill(0.0)),
            ],
            10.0,
        ));
        content = content.push(legend(
            accent_fill(small()),
            tr("LLM models"),
            format!("{} GiB", gib(n(stats, "llm_vram"))),
            tr("↑ this number is shown in the bar"),
            None,
        ));

        // The two buffers that fill while you work: the context (whole
        // conversation) and the reply being generated.
        let context = n(p, "context");
        if let Some(used) = num(p, "context_used").filter(|_| context > 0.0) {
            let filled = used / context;
            let memory = n(p, "context_memory");
            content = content.push(legend(
                fill(level_color(filled), small()),
                tr("Context buffer"),
                if memory > 0.0 {
                    trf("≈ {0} of {1}", &[mib(memory * filled), mib(memory)])
                } else {
                    format!("{}%", (filled * 100.0).round())
                },
                trf("{0} / {1} tokens ({2}%)", &[tokens(used), tokens(context), (filled * 100.0).round().to_string()]),
                Some(filled),
            ));
        }
        let limit = n(p, "output_limit");
        if limit > 0.0 {
            let used = n(p, "output_used");
            let filled = used / limit;
            let memory = n(p, "context_memory");
            let generating = p.get("generating").and_then(Value::as_bool) == Some(true);
            content = content.push(legend(
                fill(level_color(filled), small()),
                tr("Output buffer"),
                if memory > 0.0 && context > 0.0 {
                    format!("≈ {}", mib(memory * used / context))
                } else {
                    format!("{}%", (filled * 100.0).round())
                },
                if generating { trf("generating {0} / {1} tokens ({2}%)", &[tokens(used), tokens(limit), (filled * 100.0).round().to_string()]) } else { trf("last reply {0} / {1} tokens ({2}%)", &[tokens(used), tokens(limit), (filled * 100.0).round().to_string()]) },
                Some(filled),
            ));
        }
        let apps: Vec<String> = stats
            .get("other_apps")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|app| format!("{} {}", s(app, "name"), gib(n(app, "vram")))).collect())
            .unwrap_or_default();
        content = content.push(legend(
            neutral_fill(small()),
            tr("Desktop & apps"),
            format!("{} GiB", gib(n(stats, "other_vram"))),
            apps.join(" · "),
            None,
        ));
        content = content.push(legend(track(small()), tr("Free"), format!("{} GiB", gib(free)), String::new(), None));
        let spilled = n(stats, "llm_spilled");
        if spilled > SPILL_SHOWN {
            content = content.push(
                text::caption(trf("⚠ {0} GiB of model memory does not fit in VRAM and was moved to system RAM. Generation is much slower; unload other models or free GPU memory.", &[gib(spilled)]))
                .class(theme::Text::Color(level_color(0.8))),
            );
        }

        // GPU activity and generation speed.
        content = content.push(section(tr("GPU activity")));
        let mut load = pct(num(gpu, "busy"));
        if let Some(temp) = num(gpu, "temp").filter(|t| *t > 0.0) {
            load += &format!("  ·  {} °C", temp.round());
        }
        if let Some(power) = num(gpu, "power").filter(|w| *w > 0.0) {
            load += &format!("  ·  {} W", power.round());
        }
        content = content.push(meter(tr("Load"), load, n(gpu, "busy") / 100.0));
        if let Some(speed) = p.get("speed").filter(|v| v.is_object()) {
            let avg = num(speed, "avg");
            let max = num(speed, "max");
            let now = num(speed, "now");
            let stat = if n(speed, "count") > 0.0 {
                format!("  ·  {}", trf("min {0}  avg {1}  max {2}", &[rate(num(speed, "min")), rate(avg), rate(max)]))
            } else if avg.is_some_and(|a| a > 0.0) {
                format!("  ·  {}", trf("avg {0} since load", &[rate(avg)]))
            } else {
                String::new()
            };
            let value = match now {
                Some(v) => trf("{0} tok/s", &[rate(Some(v))]),
                None if avg.is_some_and(|a| a > 0.0) => tr("idle"),
                None => String::new(),
            };
            let fraction = match now {
                Some(v) if v > 0.0 => v / v.max(max.unwrap_or(0.0)),
                _ => 0.0,
            };
            content = content.push(meter(format!("{}{stat}", tr("Speed")), value, fraction));
        }

        content = content.push(section(tr("System")));
        content = content.push(meter(tr("CPU"), pct(num(system, "cpu")), n(system, "cpu") / 100.0));
        let mem_total = n(system, "mem_total");
        content = content.push(meter(
            tr("RAM"),
            format!("{} / {} GiB", gib(n(system, "mem_used")), gib(mem_total)),
            if mem_total > 0.0 { n(system, "mem_used") / mem_total } else { 0.0 },
        ));

        // One block per runtime.
        content = content.push(divider::horizontal::default());
        content = content.push(section(tr("Runtimes")));
        if let Some(runtimes) = stats.get("runtimes").and_then(Value::as_array) {
            for runtime in runtimes {
                content = content.push(self.runtime_block(runtime));
            }
        } else {
            content = content.push(dim(tr("Waiting for the collector…")));
        }
        content.spacing(8).into()
    }
}

impl<const WINDOW_MODE: bool> cosmic::Application for Monitor<WINDOW_MODE> {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    // Wayland associates the window with its launcher entry and icon by this ID.
    const APP_ID: &'static str = if WINDOW_MODE {
        "io.github.ubruckhaus.CosmicMaprismaLlmMonitor.Window"
    } else { APP_ID };

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: ()) -> (Self, Task<Message>) {
        let window_mode = WINDOW_MODE;
        let mut app = Self { core, window_mode, shot_key: shot_key_setting(), corners: corners_setting(), ..Default::default() };
        if window_mode { app.resize = window_size::Debounce::new(window_start_size()); }
        corners::set_current(&app.corners);
        let task = if window_mode {
            app.set_header_title(tr("Maprisma LLM Monitor"));
            let id = app.core.main_window_id().unwrap();
            // The desktop's theme for the whole window (title bar too), see look.rs.
            Task::batch([app.set_window_title(tr("Maprisma LLM Monitor"), id), corners::set_app_theme(&app.corners)])
        } else { Task::none() };
        (app, task)
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::CloseRequested(id))
    }

    fn on_window_resize(&mut self, id: window::Id, width: f32, height: f32) {
        if self.window_mode && self.core.main_window_id() == Some(id) {
            self.resize.resized(width, height, std::time::Instant::now());
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            Subscription::run_with("llm-monitor-collector", |_| {
                stream::channel(4, |mut output| async move {
                    run_collector(&mut output).await;
                })
            }),
        ];
        if self.window_mode {
            subscriptions.push(cosmic::iced::time::every(Duration::from_millis(150)).map(|_| Message::CheckClose));
            if self.resize.waiting() {
                subscriptions.push(cosmic::iced::time::every(Duration::from_millis(100)).map(|_| Message::SaveSize));
            }
        }
        // F12 (no modifiers) in the popup or window; Esc closes the innermost thing (esc_step).
        subscriptions.push(cosmic::iced::event::listen_with(|event, status, id| match event {
            cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if shot::is_shot_key(&key, modifiers) {
                    Some(Message::ShotKey(id))
                } else if status == cosmic::iced::event::Status::Ignored
                    && matches!(key, cosmic::iced::keyboard::Key::Named(cosmic::iced::keyboard::key::Named::Escape))
                {
                    // Not one an open drop-down list took to close itself.
                    Some(Message::Escape(id))
                } else {
                    None
                }
            }
            _ => None,
        }));
        if !self.window_mode {
            subscriptions.insert(0, rectangle_tracker_subscription(0).map(|update| Message::Rectangle(update.1)));
        }
        if self.busy() {
            subscriptions.push(cosmic::iced::time::every(Duration::from_millis(650)).map(|_| Message::Pulse));
        }
        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Language(code) => { self.language_error = set_language(&code).is_err(); self.language_open = false; }
            Message::ToggleLanguage => { self.language_open = !self.language_open; self.corners_open = false; }
            Message::ShotKey(id) => {
                let shown = if self.window_mode { self.core.main_window_id() == Some(id) } else { self.popup == Some(id) };
                if self.shot_key && shown && self.shot == shot::Phase::Idle {
                    self.shot = shot::Phase::Busy;
                    self.capturing = true;
                    return window::screenshot(id).map(|picture| cosmic::Action::App(Message::Captured(picture)));
                }
            }
            Message::Captured(picture) => {
                if !std::mem::take(&mut self.capturing) { return Task::none(); }
                let (width, height) = (picture.size.width, picture.size.height);
                return off_thread(move || shot::save_screenshot(SHOT_FOLDER, &picture.rgba, width, height), Message::ShotSaved);
            }
            Message::ShotSaved(result) => {
                self.shot = match result {
                    Ok(saved) => shot::Phase::Note { path: saved.path, shown: saved.shown, text: String::new() },
                    Err(reason) => shot::Phase::Error(trf("The screenshot could not be saved: {0}", &[reason])),
                };
                if matches!(self.shot, shot::Phase::Note { .. }) {
                    return cosmic::widget::text_input::focus(NOTE_ID.clone());
                }
            }
            Message::NoteInput(value) => {
                if let shot::Phase::Note { text, .. } = &mut self.shot {
                    *text = value.chars().take(shot::NOTE_LIMIT).collect();
                }
            }
            Message::NoteDone => return self.finish_note(true),
            Message::Escape(id) => {
                let shown = if self.window_mode { self.core.main_window_id() == Some(id) } else { self.popup == Some(id) };
                match esc_step(self.shot.open(), self.language_open || self.corners_open, shown) {
                    EscStep::Note => return self.finish_note(false),
                    EscStep::Menus => { self.language_open = false; self.corners_open = false; }
                    // Like the window's own close (the tray keeps running); the popup like a click on the icon.
                    EscStep::Close if self.window_mode => { self.save_size(true); return cosmic::iced::exit(); }
                    EscStep::Close => {
                        if let Some(popup) = self.popup.take() {
                            return Task::batch([self.popup_closed(), destroy_popup(popup)]);
                        }
                    }
                    EscStep::Nothing => {}
                }
            }
            Message::NoteSaved(result) => match result {
                Ok(()) => { if self.shot == shot::Phase::Noting { self.shot = shot::Phase::Idle; } }
                Err(reason) => self.shot = shot::Phase::Error(trf("The note could not be saved: {0}", &[reason])),
            },
            Message::SetShotKey(on) => {
                self.setting_error = write_pref("screenshot_key", Some(on.into())).is_err();
                self.shot_key = on;
            }
            Message::ToggleCorners => { self.corners_open = !self.corners_open; self.language_open = false; }
            Message::Corners(choice) => {
                let choice = corners::normalize(&choice);
                // "Like the desktop" is the default: no key in config.json.
                let value = (choice != "desktop").then(|| Value::from(choice));
                self.setting_error = write_pref("corners", value).is_err();
                self.corners = choice.to_string();
                corners::set_current(choice);
                self.corners_open = false;
                if self.window_mode { return corners::set_app_theme(choice); }
            }
            Message::CheckClose => {
                if window_mode::CLOSE.load(Ordering::Relaxed) { self.save_size(true); return cosmic::iced::exit(); }
            }
            Message::SaveSize => self.save_size(false),
            Message::Stats(stats) => {
                self.stats = stats;
                // Settings changed in another interface (window, applet) take effect here too.
                self.shot_key = shot_key_setting();
                let corners = corners_setting();
                let mut restyle = corners != self.corners;
                if restyle { corners::set_current(&corners); self.corners = corners; }
                // The desktop's colours and corners (Maprisma Theme can change them any time).
                restyle |= maprisma_llm_monitor::look::refresh();
                if restyle && self.window_mode {
                    if !self.busy() { self.pulse_dim = false; }
                    return corners::set_app_theme(&self.corners);
                }
                if !self.busy() {
                    self.pulse_dim = false;
                }
            }
            Message::Pulse => self.pulse_dim = !self.pulse_dim,
            Message::Rectangle(update) => match update {
                RectangleUpdate::Rectangle(r) => self.rectangle = r.1,
                RectangleUpdate::Init(tracker) => self.tracker = Some(tracker),
            },
            Message::CloseRequested(id) => {
                if self.window_mode { self.save_size(true); }
                if self.popup == Some(id) {
                    self.popup = None;
                    return self.popup_closed();
                }
            }
            Message::TogglePopup => {
                if self.window_mode { return Task::none(); }
                if let Some(popup) = self.popup.take() {
                    return Task::batch([self.popup_closed(), destroy_popup(popup)]);
                }
                let focus = if matches!(self.shot, shot::Phase::Note { .. }) {
                    cosmic::widget::text_input::focus(NOTE_ID.clone())
                } else {
                    Task::none()
                };
                return Task::batch([cosmic::surface::surface_task(cosmic::surface::action::app_popup(
                    |_| Default::default(),
                    |app: &mut Self| {
                        let id = window::Id::unique();
                        app.popup = Some(id);
                        let mut settings =
                            app.core.applet.get_popup_settings(app.core.main_window_id().unwrap(), id, None, None, None);
                        let Rectangle { x, y, width, height } = app.rectangle;
                        settings.positioner.anchor_rect = Rectangle::<i32> {
                            x: x.max(1.0) as i32,
                            y: y.max(1.0) as i32,
                            width: width.max(1.0) as i32,
                            height: height.max(1.0) as i32,
                        };
                        settings.positioner.size = None;
                        settings
                    },
                    None,
                )), focus]);
            }
            Message::FreeGpu => {
                // Stops the llama-server, Ollama and vLLM user services and unloads
                // LM Studio's models; one run at a time, never cut short.
                if let Some(script) = script_path("free-gpu.sh")
                    && !FREEING.swap(true, Ordering::SeqCst)
                {
                    spawn_detached(vec!["bash".into(), script.into_os_string().into_string().unwrap_or_default()], true);
                }
            }
            Message::OpenBtop => {
                spawn_detached(vec!["xdg-terminal-exec".into(), "btop".into()], false);
                if let Some(popup) = self.popup.take() {
                    return Task::batch([self.popup_closed(), destroy_popup(popup)]);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        if self.window_mode { return corners::apply_window(&self.corners, self.popup_content()); }
        let horizontal = matches!(self.core.applet.anchor, PanelAnchor::Top | PanelAnchor::Bottom);
        let padding = self.core.applet.suggested_padding(true).0;
        let button = button::custom(self.panel_content())
            .padding(if horizontal { [0, padding] } else { [padding, 0] })
            .on_press_down(Message::TogglePopup)
            .class(theme::Button::AppletIcon);
        // Right click: btop.
        let button = mouse_area(button).on_right_press(Message::OpenBtop);
        let content: Element<Message> = match self.tracker.as_ref() {
            Some(tracker) => tracker.container(0, button).ignore_bounds(true).into(),
            None => button.into(),
        };
        autosize::autosize(content, AUTOSIZE_ID.clone()).into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Message> {
        if self.window_mode { return corners::apply_window(&self.corners, self.popup_content()); }
        corners::apply(&self.corners, self.core.applet.popup_container(self.popup_content()))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        if self.window_mode { None } else { Some(cosmic::applet::style()) }
    }
}

impl<const WINDOW_MODE: bool> Monitor<WINDOW_MODE> {
    // Done, Enter or the popup closing (keep: the note typed so far is saved), Esc (skip).
    fn finish_note(&mut self, keep: bool) -> Task<Message> {
        match std::mem::take(&mut self.shot) {
            shot::Phase::Note { path, text, .. } => {
                if keep && shot::note_text(&text).is_some() {
                    // Busy until the note is written, so a new F12 cannot replace it.
                    self.shot = shot::Phase::Noting;
                    return off_thread(
                        move || shot::save_note(&path, SHOT_FOLDER, &text).map(|_| ()).map_err(|e| e.to_string()),
                        Message::NoteSaved,
                    );
                }
            }
            shot::Phase::Error(_) => {}
            other => self.shot = other,
        }
        Task::none()
    }

    // A capture still on its way is dropped with its popup; a typed note is kept (saved).
    // A resize that has settled (or, on `closing`, any waiting one) goes to config.json; not
    // while maximised, fullscreen or tiled (libcosmic's sharp corners).
    fn save_size(&mut self, closing: bool) {
        let sharp = self.core.window.sharp_corners;
        let ignore = || window_size::not_users_size(sharp);
        let size = if closing { self.resize.flush(ignore) } else { self.resize.due(std::time::Instant::now(), ignore) };
        if let Some(size) = size {
            self.setting_error = write_pref(window_size::KEY, Some(window_size::to_value(size))).is_err();
        }
    }

    fn popup_closed(&mut self) -> Task<Message> {
        if std::mem::take(&mut self.capturing) && self.shot == shot::Phase::Busy {
            self.shot = shot::Phase::Idle;
        }
        if matches!(self.shot, shot::Phase::Note { .. }) { self.finish_note(true) } else { Task::none() }
    }

    // After F12: "Screenshot saved", where, the optional note and Done (or the error and Done).
    fn shot_overlay(&self) -> Option<Element<'_, Message>> {
        let body: Element<Message> = match &self.shot {
            shot::Phase::Note { shown, text: note, .. } => column![
                text::heading(tr("Screenshot saved")),
                dim(trf("Saved as {0}", &[shown.clone()])),
                cosmic::widget::text_input(tr("What is wrong? (optional)"), note.as_str())
                    .id(NOTE_ID.clone())
                    .on_input(Message::NoteInput)
                    .on_submit(|_| Message::NoteDone),
                row![space::horizontal().width(Length::Fill), button::suggested(tr("Done")).on_press(Message::NoteDone)],
            ]
            .spacing(10)
            .into(),
            shot::Phase::Error(reason) => column![
                text::body(reason.clone()),
                row![space::horizontal().width(Length::Fill), button::suggested(tr("Done")).on_press(Message::NoteDone)],
            ]
            .spacing(10)
            .into(),
            _ => return None,
        };
        let card = container(body).padding(16).width(Length::Fill).class(theme::Container::Card);
        let card = container(card).padding(12).width(if self.window_mode { Length::Fill } else { Length::Fixed(POPUP_WIDTH) });
        Some(card.into())
    }

    fn popup_content(&self) -> Element<'_, Message> {
        if let Some(overlay) = self.shot_overlay() {
            return overlay;
        }
        let mut actions = column![
            divider::horizontal::default(),
            menu_button(text::body(tr("Free GPU memory"))).on_press_maybe(self.active().then_some(Message::FreeGpu)),
            menu_button(text::body(tr("Open btop"))).on_press(Message::OpenBtop),
        ];
        actions = actions.push(menu_button(text::body(tr("Language"))).on_press(Message::ToggleLanguage));
        if self.language_error { actions = actions.push(text::body(tr("Could not save the language setting."))); }
        if self.language_open {
            for (code, label) in language_options() {
                actions = actions.push(menu_button(text::body(label)).on_press(Message::Language(code)));
            }
            return scrollable(container(actions).padding(8)).height(Length::Fixed(460.0)).into();
        }
        actions = actions.push(padded_control(
            row![
                text::body(tr("F12 saves a screenshot")),
                space::horizontal().width(Length::Fill),
                cosmic::widget::toggler(self.shot_key).on_toggle(Message::SetShotKey),
            ]
            .align_y(Alignment::Center),
        ));
        actions = actions.push(menu_button(text::body(tr("Corners"))).on_press(Message::ToggleCorners));
        if self.corners_open {
            let desktop = corners::desktop_radius().round().to_string();
            for (choice, label) in [
                ("desktop", trf("Like the desktop ({0} px)", &[desktop])),
                ("slight", trf("Slightly rounded ({0} px)", &["6".into()])),
                ("rounded", trf("Rounded ({0} px)", &["12".into()])),
                ("square", tr("Square")),
            ] {
                let label = if self.corners == choice { format!("✓ {label}") } else { label };
                actions = actions.push(menu_button(text::body(label)).on_press(Message::Corners(choice.into())));
            }
        }
        if self.setting_error { actions = actions.push(text::body(tr("Could not save the setting."))); }
        let detail_content = if self.window_mode { container(self.details()).padding([12, 16]).width(Length::Fill) } else { padded_control(self.details()).padding([12, 16]) };
        let details = scrollable(detail_content).height(if self.window_mode { Length::Fill } else { Length::Shrink });
        let content = column![container(details).max_height(POPUP_MAX_HEIGHT), actions]
            .width(if self.window_mode { Length::Fill } else { Length::Fixed(POPUP_WIDTH) })
            .padding([0, 0, 8, 0]);
        content.into()
    }
}

// ------------------------------------------------------------------------- processes

// Streams the collector's snapshots; starts it again RESTART_DELAY after it exits.
async fn run_collector(output: &mut mpsc::Sender<Message>) {
    loop {
        if let Some(script) = script_path("llm-monitor.py") {
            let child = tokio::process::Command::new("python3")
                .arg("-I")
                .arg(&script)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn();
            if let Ok(mut child) = child {
                if let Some(stdout) = child.stdout.take() {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        // A partial line while the collector exits is skipped.
                        if let Ok(stats) = serde_json::from_str::<Value>(&line)
                            && stats.get("error").is_none()
                            && output.send(Message::Stats(stats)).await.is_err()
                        {
                            return;
                        }
                    }
                }
                let _ = child.kill().await;
            }
        }
        tokio::time::sleep(RESTART_DELAY).await;
    }
}

// Runs argv without waiting for it; a thread reaps it. free_gpu: clears the
// one-run-at-a-time flag once it ends.
fn spawn_detached(argv: Vec<String>, free_gpu: bool) {
    let mut command = std::process::Command::new(&argv[0]);
    if free_gpu {
        if let Ok(exe) = std::env::current_exe() { command.env("MAPRISMA_NOTIFY", exe); }
    }
    let child = command
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
                if free_gpu {
                    FREEING.store(false, Ordering::SeqCst);
                }
            });
        }
        Err(_) => {
            if free_gpu {
                FREEING.store(false, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_closes_the_innermost_thing_first() {
        assert_eq!(esc_step(true, true, true), EscStep::Note);
        assert_eq!(esc_step(false, true, true), EscStep::Menus);
        assert_eq!(esc_step(false, false, true), EscStep::Close);
        // Not from the window or the open popup (e.g. the panel button): nothing.
        assert_eq!(esc_step(true, true, false), EscStep::Nothing);
        assert_eq!(esc_step(false, false, false), EscStep::Nothing);
    }
}
