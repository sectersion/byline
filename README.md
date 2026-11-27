<div align="center">

# byline

**Your hours, tracked securely.**

byline sits between your editor and Hackatime — invisible, passive, local —
and counts human involvement around AI, not agent runtime.

Then it builds the evidence a reviewer needs to say
*"yes, these hours are real"* — in minutes, not courtrooms.

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

## how it works

```
 editor ──heartbeats──▶ byline daemon ──untouched──▶ Hackatime
                          │  local sqlite
                          ▼
                     byline report
              hours · struggle · agent time
```

Your editor sends the heartbeats it always sent. byline stores them raw,
forwards them untouched, and derives the human story from the AI and human
fields already in every heartbeat.

<br>

## status

design phase — the wire contract and the review artifact are specified;
the daemon is next.

<br>

AGPL-3.0

</div>
