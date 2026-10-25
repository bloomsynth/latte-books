# Benchmarks

How Latte Books compares with Frappe Books, the Electron app it ports, and how each number
was measured.

Both apps were measured on 2026-10-25, on the same machine and opening the same company
file to its Get Started screen: Latte Books 0.1.0-beta.1 and Frappe Books v0.36.0, each the
installed release build. The machine is a desktop with an AMD Ryzen 7 3700X, an NVIDIA
RTX 2060 and an SSD, on Windows 11.

## Startup

From launch until the window is visible, and until it is ready to use. Median of 7
launches each.

| | Window visible | Ready to use |
|---|---|---|
| **Latte Books** | **0.48 s** | **0.79 s** |
| Frappe Books | 4.14 s | 7.12 s |
| | 8.6× sooner | **9× sooner** |

Latte's launches all fell between 0.76 and 0.81 s. Frappe Books' ranged from 5.7 to 10.0 s.

- **Method:** [`spikes/gpui-startup/measure-visual.ps1`](../spikes/gpui-startup/measure-visual.ps1)
  launches the app and screenshots its window every few milliseconds. *Ready* is the last
  time the window's pixels changed, followed by 2 seconds with no change, so a blank window
  or a spinner counts as still loading.
- **Warm start.** Launches follow each other, so the apps' files are in the disk cache.
  About 0.28 s of each *ready* time is the Windows open animation.

## Memory

Read 15 seconds after launch, with the company open, summed over all of each app's
processes. Median of 3 launches each.

| | Latte Books | Frappe Books | |
|---|---|---|---|
| Processes | **1** | 5 | |
| Private working set (Task Manager's *Memory* column) | **18 MB** | 150 MB | 8× less |
| Committed (private bytes) | **112 MB** | 292 MB | 2.6× less |
| Working set, summed | **66 MB** | 388 MB | 5.9× less |

- **Private working set** is the memory only that app is using, and is what Task Manager
  shows.
- **Committed** memory includes what is reserved but not currently in RAM. It is the most
  conservative of the three comparisons.
- **The summed working set** counts memory shared between processes once per process, which
  overstates Electron's five processes. It is listed for completeness.

## Size

| | Latte Books | Frappe Books | |
|---|---|---|---|
| Installed on Windows | **87 MB** | 249 MB | 2.9× smaller |
| Windows x64 installer | **25.1 MB** | 75.6 MB | 3.0× smaller |
| macOS installer, Apple silicon | **35.0 MB** | 95.9 MB | 2.7× smaller |
| Linux AppImage | **43.0 MB** | 104.0 MB | 2.4× smaller |
| Linux .deb | **45.9 MB** | 73.0 MB | 1.6× smaller |

Installer sizes are from each project's releases page: Latte Books
[0.1.0-beta.1](https://github.com/bloomsynth/latte-books/releases/tag/v0.1.0-beta.1) and
Frappe Books [v0.36.0](https://github.com/frappe/books/releases/tag/v0.36.0). Latte's
installed size includes `latte-mcp`, the server AI assistants run.

## Run it yourself

From PowerShell, with the app installed:

```powershell
spikes\gpui-startup\measure-visual.ps1 -Exe "$env:LOCALAPPDATA\Latte Books\latte-books.exe" -Runs 7
```

Pass Frappe Books' `Frappe Books.exe` to `-Exe` to time it the same way. Both apps open the
company used last, so open the same file in each first. For memory, open Task Manager's
**Details** tab, or sum `WorkingSetPrivate` from `Win32_PerfFormattedData_PerfProc_Process`
over each app's processes.

## Why it's faster

Frappe Books is an Electron app: each launch starts Chromium and Node.js in several
processes, then loads and runs the Vue interface as JavaScript. Latte Books is one native
program, written in Rust and drawn on the GPU by [GPUI](https://www.gpui.rs/), the UI
framework of the Zed editor. In the prototype, about half of the startup time was the
graphics driver setting up the GPU.

The first measurements, made on a prototype before the app was built, are in
[spikes/gpui-startup/RESULTS.md](../spikes/gpui-startup/RESULTS.md).
