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

## Install

Needs `python3` (standard library only), Rust (cargo) and COSMIC. btop is optional.

```sh
./install.sh
```

Then add **Maprisma LLM Monitor** in COSMIC Settings › Desktop › Panel › Applets.

## Tests

```sh
env -u DISPLAY -u WAYLAND_DISPLAY python3 -m unittest discover -s tests
```

MIT license.
