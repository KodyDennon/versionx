---
title: Exit codes
description: Every exit code Versionx can return, with semantics and recovery advice.
sidebar_position: 8
---

# Exit codes

:::info
The table below is **auto-generated** from the `versionx-core` error taxonomy. Edit error variants there and re-run `cargo xtask docs-exit-codes`.
:::

| Code | Meaning |
|---|---|
| `0` | Success. |
| `1` | Generic user error (bad argument, missing file, etc.). Message on stderr describes it. |
| `2` | Config error. `versionx.toml` is missing, unparseable, or fails schema validation. |
| `3` | Policy violation. At least one `deny` rule fired and no waiver applied. Rerun with `--explain` for specifics. |
| `4` | Network or I/O error. Check `VERSIONX_LOG=debug` output. |
| `5` | Prerequisite mismatch during `apply`. The state of the world moved since the plan was generated — regenerate the plan. |
| `6` | Git error. Non-zero exit from git, merge conflict, missing remote, etc. |
| `7` | Daemon unavailable and `--no-daemon` was not set. |
| `8` | Waiver expired. A required waiver has passed its `expires` date. |
| `9` | Saga failure. Multi-repo operation failed mid-flight. `versionx saga status` for recovery. |
| `10+` | Subsystem-specific. See auto-generated detail below. |

## In scripts

```bash
versionx release plan > plan.json
if [ $? -eq 3 ]; then
    echo "Policy violation. See above."
    exit 1
fi
```

## Auto-generated detail

{/* GENERATED-BELOW */}

_Subsystem exit codes pending first generation. Run `cargo xtask docs-exit-codes`._

{/* GENERATED-ABOVE */}

## See also

- [Environment variables](./environment-variables) — turn on `VERSIONX_LOG=debug` when diagnosing.
- [Debugging & tracing](/contributing/debugging-and-tracing) — for contributors adding new error codes.
