# System Observatory

A unified control room for the machine, built entirely on telemetry Vanta
already collects. No mock data: every number comes from the host's
`vanta_query` API, and anything the host cannot see is named rather than
invented.

```
╭ system observatory ────────────────────────╮╭ health ──────────────────────╮
│cpu   ▅▂▃▁▂▂▂▁  20%                         ││● DEGRADED                    │
│mem   ▄▄▄▄▄▄▄▄  51%                         ││                              │
│disk  ▇▇▇▇▇▇▇▇  88%                         ││warn thermal 82°C             │
│net ↓  █▅▄▆▄▃▄ 559K/s                       ││warn disk    / at 88%         │
│net ↑          24K/s                        ││                              │
│                                            ││unmonitored: network.{interfa…│
│load  5.89 5.51 4.04  /8  82°C              │╰──────────────────────────────╯
│procs 164  top Telegram 29%                 │
│swap  3.5G / 14.0G                          │
│! disk / at 88%                             │
╰────────────────────────────────────────────╯
```

## Requirements

**Vanta >= 0.10.26.** This extension imports the `vanta_query` host function;
on older hosts an unknown import makes the plugin fail to load. That
requirement is encoded as `api_version = "0.9.2"`.

## Install

```bash
vanta ext install system_observatory
```

Then enable it and place its widgets:

```toml
[extensions]
enabled = ["system_observatory"]

[dashboard]
layout = [
    ["system_observatory", "system_health", "cpu"],
    ["resource_timeline", "resource_flow", "load_history"],
]
```

## Widgets

| id | shows |
|---|---|
| `system_observatory` | cpu, memory, disk, network with trend sparklines; load, process count and busiest process, swap, and the worst current health finding |
| `system_health` | overall verdict plus every subsystem over threshold, with the measured value; also lists what the host does **not** monitor |
| `resource_timeline` | wide rolling history for cpu / mem / disk / rx / tx with a time axis |
| `resource_flow` | braille flow view — twice the time span of the block graphs in the same width |
| `load_history` | load 1/5/15, load per core, and a trend where 100% marks saturation |

All five share one cached telemetry snapshot per frame, so panels can never
disagree with each other, and the host is queried once rather than once per
widget.

## Health thresholds

| subsystem | warn | critical |
|---|---|---|
| cpu | 85% | 95% |
| memory | 85% | 95% |
| swap | 50% | — |
| disk (per mount) | 85% | 95% |
| thermal | 80°C | 90°C |
| load (per core) | 1.0 | 2.0 |

Load is normalised by core count, so the verdict means the same thing on a
4-core laptop and a 64-core server.

## Not monitored

The host does not collect these, so the extension reports them as unmonitored
instead of guessing: per-interface network counters, socket/connection tables,
containers, git state, and the system journal.

## Build from source

```bash
cargo build -p system-observatory --target wasm32-wasip1 --release
cp target/wasm32-wasip1/release/system_observatory.wasm ~/.config/vanta/extensions/
```
