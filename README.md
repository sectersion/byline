<div align="center">

# byline

**Your hours, tracked securely.**

[![license: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![rust: 2021](https://img.shields.io/badge/rust-2021-orange.svg)](https://www.rust-lang.org)
[![platform: macOS • Linux • Windows](https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey.svg)]()

<br>

byline sits between your editor and Hackatime — passive, local, invisible —
and counts human involvement around AI, not agent runtime.

<br>

> ### byline makes faking expensive and visible, not impossible.
> **The reviewer, not the instrument, is the fraud control.**

<br>

| | |
|:---:|:---|
| **Passive** | no timers, no start/stop, no self-reporting |
| **Local first** | logged activity never leaves your machine |
| **Compatible** | speaks WakaTime; Hackatime keeps every hour |
| **Honest** | stuck-and-resolved counts as the work it is |

<br>

## editors

| editor | status |
|:---:|:---:|
| 🤖 **Claude Code** | supported |
| ⚡ **OpenCode** | supported |

<br>

## install

```sh
cargo install --path .
```

<br>

## commands

### `byline setup`
Repoint `~/.wakatime.cfg` at the local daemon and install editor hooks.

```sh
byline setup            # install
byline setup --no-plugins   # skip editor hook install
```

### `byline serve`
Run the local ingest daemon on `127.0.0.1:8787` (override with `BYLINE_PORT`).

```sh
byline serve
```

### `byline status`
One line: row count, last-heartbeat age, unforwarded count, unknown paths.

```sh
byline status
```

### `byline report`
Hours per local day per project, derived from stored heartbeats.

```sh
byline report
```

### `byline export`
JSONL export of stored raw heartbeats.

```sh
byline export
```

### `byline unsetup`
Restore `~/.wakatime.cfg` from the backup.

```sh
byline unsetup
```

<br>

AGPL-3.0

</div>