# Maprisma LLM Monitor for COSMIC

**See at a glance when a local LLM is using your GPU, how much it holds, how full its context is, and who is using it.**

A native COSMIC panel applet (Rust/libcosmic) for people who run local models with **llama.cpp**, **Ollama**,
**LM Studio** or **vLLM**. It is one of the [Maprisma](https://github.com/maprisma) apps and the COSMIC port of
the Omarchy bar widget [LLM Monitor](https://github.com/uBruckhaus/omarchy-llm-usage), which stays as it is. It
uses the same collector. English only.

- **Panel icon:** a chip. It is dim while no model is loaded. While a model is resident, it shows the GPU memory
  held by local models and how full the model's context is, with a thin fill line underneath, coloured from
  green to red as the context fills. It pulses while the GPU is generating.
- **Details (left click):**
  - **GPU memory:** the share held by LLM models, by the desktop and apps, and what is free.
  - **Context buffer:** the tokens in the context window and the share of the KV cache they fill.
  - **Output buffer:** the tokens of the reply being generated, against its limit.
  - **GPU activity:** load, temperature and power, plus the generation speed in tokens per second.
  - **System:** CPU and RAM.
  - **Runtimes:** llama.cpp, Ollama, LM Studio and vLLM, each with its state, model, context fill, VRAM, CPU, RAM
    and the programs connected to it.
  - **Free GPU memory** and **Open btop**.
- **Right click:** opens btop.

The data comes from `scripts/llm-monitor.py`, which streams one JSON snapshot every two seconds. It only reads
`/proc`, sysfs and the runtimes' local APIs. It never starts or stops a runtime on its own. Only **Free GPU
memory** (`scripts/free-gpu.sh`) does, when you click it: it stops the runtimes' systemd services, while LM
Studio keeps running and unloads its models.

## F12: a screenshot with a note

While the popup (or the KDE window) is open, **F12** (without Ctrl, Shift, Alt or Super) saves a picture of it,
for telling what is wrong with it:

- **Only on F12.** Nothing is captured at any other time, and only this app's own popup or window: libcosmic draws
  it once more into memory (iced's `window::screenshot`) and the app encodes that as PNG. No screen capture tool
  or portal runs, nothing else on the screen is in it, and nothing is uploaded or sent anywhere.
- **Where:** `~/Pictures/Maprisma LLM Monitor/maprisma-llm-monitor-<date>-<time>.png` (the pictures folder from
  `~/.config/user-dirs.dirs` when it is set there; that file is only read when it is a small regular file). The
  popup then names the file and asks **What is wrong? (optional)**; **Enter** or **Done** saves the text next to
  the picture as a `.txt` with the same name (at most 4000 characters; no text, no file), **Esc** skips it, and
  closing the popup keeps what was typed.
- **Private files:** the folder is made `0700` when missing and only used when it is a real folder (not a link) of
  yours that no one else can write to; pictures and notes are new `0600` files created inside that folder,
  never written through a link or over an existing file or note (`-2`, `-3` … are added instead), and a file that
  cannot be written completely is removed again.
- **Off:** switch **F12 saves a screenshot** off in the popup (stored as `"screenshot_key": false` in
  `config.json`, see below).

The KDE tray icon's menu is drawn by Plasma and has no F12.

## Corners

**Corners** in the popup or window: *Like the desktop (… px)* (the default: the COSMIC theme's corner radius, 16,
8 or 2 px by COSMIC Settings › Appearance; on KDE the theme libcosmic loads there, normally 16 px), *Slightly
rounded (6 px)*, *Rounded (12 px)* or *Square*. Cards and the popup's frame take the full radius, buttons, menu
rows and fields at most 8 px, small parts (meter tracks, fills, legend swatches) at most 4 px; the runtime dots and
the status pill stay round, or square in *Square*. The panel icon and the KDE window's own frame keep the
desktop's corners. Stored as `"corners"` in `config.json`; *Like the desktop* removes it again.

## Install

Needs `python3` (standard library only), Rust (cargo), and COSMIC or KDE Plasma. btop is optional.

```sh
./install.sh
```

Then add **Maprisma LLM Monitor** in COSMIC Settings › Desktop › Panel › Applets.

## Tests

```sh
env -u DISPLAY -u WAYLAND_DISPLAY python3 -m unittest discover -s tests
```

MIT license.

## KDE Plasma

On KDE Plasma 6.8 (Wayland), run `./install.sh`, then open **Maprisma LLM Monitor**
from the application launcher. `maprisma-llm-monitor --window` opens the same details
and actions as the COSMIC popup in a resizable window. Only one window runs per session.
The window uses the installed application's icon and title.

The installer also installs `maprisma-llm-monitor-tray` and a KDE-only autostart entry
(`~/.config/autostart/io.github.ubruckhaus.CosmicMaprismaLlmMonitor.Tray.desktop`,
or under `$XDG_CONFIG_HOME`). Start the tray manually for the current session, or log
out and back in. Left click toggles the window; the menu offers **Open Maprisma LLM
Monitor** and **Quit tray**. Quitting the tray leaves an open window running. The tray
reconnects when Plasma's StatusNotifierWatcher restarts and allows only one tray per
session. Its tooltip shows the panel's memory/context text, or “No local model loaded”.
It runs the same lightweight streaming collector, without polling extra services.
The COSMIC panel applet and shared scripts remain unchanged.

Rust checks: `cargo test` and `cargo build --release --offline`.

Interface language follows the first non-empty `LC_ALL`, `LC_MESSAGES`, or
`LANG` value, falling back to English for unknown languages. Choose **Language**
in the applet popup, window, or tray menu to select System or one of the eight
built-in languages. Changes apply immediately (other running interfaces refresh
with their next collector update). The setting is saved in
`$XDG_CONFIG_HOME/maprisma-llm-monitor/config.json`, or
`~/.config/maprisma-llm-monitor/config.json` by default. The same file holds the F12 and Corners settings;
files without them stay valid.

The English-key catalogs in `translations/` were imported from the read-only
Omarchy `I18n.js`; port-specific strings live in `translations/extra/`.
After editing catalogs, run `tools/gen-translations` to refresh the Rust assets
compiled into both binaries. No translations are downloaded. Check coverage
with `python3 -m unittest discover -s tests -p test_translations.py` and locale
fallback with `cargo test --offline`.
