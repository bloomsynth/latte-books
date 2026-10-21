# Spike 1: startup time

**Question:** how long from launch until a GPUI window with a Books-like screen is painted?
**Target (PLAN.md §1):** well under a second, even on modest hardware.

## Setup

- gpui-kit `=0.8.0` → GPUI snapshot `gpui-pre 0.3.9`; Rust 1.98.1; release build, no LTO.
- The window shows a sidebar with the nine Books v0.36.0 groups and a 40-row invoice list
  with a gpui-kit button (`src/main.rs`). `--exit` quits after the first frame.
- `measure.ps1` launches it 10 times and reports the median.

## Results — 2026-10-21

Machine: desktop, AMD Ryzen 7 3700X, NVIDIA RTX 2060 (driver 32.0.16.1692), Windows 11,
SSD. Only ~2 GB of free memory at the time, because many other apps were open.

| Measure | Median | Min | Max |
|---|---|---|---|
| `main()` → first frame presented | **543 ms** | 520 ms | 595 ms |
| Launch → process exit (wall clock) | 608 ms | 585 ms | 665 ms |

Binary size: 17.8 MB (debug info stripped).

Where the time goes (single run, ms from `main()`):

| Step | At | Took |
|---|---|---|
| Platform created (OLE, DirectWrite, hidden window) | 181 | 181 |
| DirectX 11 devices created (`attach_gpu`), app ready | 459 | ~280 |
| First render of the view | 522 | ~60 |
| Window opened, first frame presented | 533 | ~10 |

Disabling DirectComposition (`GPUI_DISABLE_DIRECT_COMPOSITION=1`) changed nothing measurable.

## Compared with Frappe Books — 2026-10-21

Both apps were measured the same way from outside with `measure-visual.ps1`, on the same machine:
5 launches each, median. The script screenshots the window every few ms. "Ready" is the last
time the pixels changed, followed by 2 s with no change, so blank windows, spinners and the
Windows open animation all count as not ready yet.

| App | Window visible | Ready (screen settled) |
|---|---|---|
| Frappe Books **0.34.0** (installed copy; Electron), opening an existing company file | 4,055 ms | **5,832 ms** |
| GPUI spike (fake data, no database) | 509 ms | **809 ms** |

**Caveats:**
- Books opened a real company file and showed the Get Started screen. The spike shows fixed
  sample data and opens no database.
- A real Latte Books also has to open SQLite and load the schemas. Both should take tens of
  milliseconds, but that hasn't been measured yet.
- Books 0.34.0 is the version installed here; v0.36.0 has the same Electron architecture.
- About 280 ms of each "Ready" figure is the Windows window-open animation, for both apps.

## Verdict

**Pass on this machine:** about 0.55 s to the first painted frame and 0.6 s wall clock.
Frappe Books took about 7× longer on the same machine (5.8 s against 0.8 s, measured the same way).

About half the time is GPU driver setup (DirectX device creation). That cost is in the driver
rather than our code, and it varies by GPU vendor.

## Still open

- **Low-end laptop** (integrated graphics, 8 GB RAM, slow disk). This is the case the plan asks for.
- **Cold start** after a reboot, when the exe and DLLs are not yet in the file cache.
- **Startup with a real company file**, once `latte-db` can open one (Phase 1).
- **The `gpui-fast` backend of gpui-kit**, which may start faster.
- **LTO and `codegen-units = 1`.** These need more free memory to build than this machine had;
  rustc crashed with `0xc0000409`. Try them on CI or with more free RAM.

## Build notes for this machine

- Build with `-j 2`. With the default 16 jobs and about 2 GB of free commit memory, rustc
  ran out of memory and wrote broken `.rmeta` files. Run `cargo clean` after that happens.
- A clean release build takes about 11 minutes with `-j 2`.
