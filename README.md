<div align="center">

# byline

**Your hours, tracked securely.**

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

## install

```
cargo install --path .
byline setup      # repoint ~/.wakatime.cfg, install editor hooks
byline serve      # start the local daemon
byline status     # one-line status
byline report     # hours per project
byline export     # JSONL export
byline unsetup    # restore ~/.wakatime.cfg
```

<br>

## editors

| editor | status |
|:---:|:---:|
| 🤖 **Claude Code** | supported |
| ⚡ **OpenCode** | supported |

<br>

AGPL-3.0

</div>