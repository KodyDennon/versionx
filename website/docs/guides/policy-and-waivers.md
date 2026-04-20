---
title: Policy & waivers
description: Declarative TOML rules, sandboxed Luau for complex logic, mandatory-expiry waivers. The unified policy surface for toolchains, deps, releases, and multi-repo.
sidebar_position: 5
---

# Policy & waivers

You'll learn:

- How to write declarative policy rules in TOML.
- When to drop to Luau for logic the DSL can't express.
- How to waive a rule temporarily with forced expiry.

**Prerequisites:** Versionx [installed](/get-started/install).

## Where policies live

```text
.versionx/
├── policies/
│   ├── releases.policy.toml
│   ├── dependencies.policy.toml
│   └── runtimes.policy.toml
└── waivers.toml
```

Fleet-wide policies live in the ops repo and inherit down; see [Multi-repo & monorepos](/guides/multi-repo-and-monorepos).

## A declarative rule

```toml
# .versionx/policies/releases.policy.toml

[rule.no-friday-majors]
when  = { action = "release", bump = "major" }
on    = { weekday = "friday", tz = "America/New_York" }
decision = "deny"
reason = "Major releases prohibited on Fridays. Cut major releases Mon–Thu or get a waiver."

[rule.require-changelog]
when = { action = "release" }
require = { has = ["changelog-entry"] }
decision = "deny-if-missing"
reason = "Every release must include a changelog entry."
```

The TOML DSL covers the 80% case: attribute matching, simple combinators, time windows.

## Dropping to Luau

For rules the DSL can't express:

```lua
-- .versionx/policies/dependencies.policy.lua

return function(ctx)
  for _, bump in ipairs(ctx.plan.dep_bumps) do
    if bump.ecosystem == "npm" and bump.kind == "major"
       and bump.package:match("^@our%-org/") then
      return {
        decision = "warn",
        reason = "Major bump on an internal package — coordinate with the core team."
      }
    end
  end
  return { decision = "allow" }
end
```

The Luau runtime is sandboxed — no filesystem access, no network, no `os.*`. The `ctx` table exposes the inputs the policy needs and nothing more.

## Evaluating policy

Automatic — every `versionx release plan`, `versionx update --plan`, and `versionx sync` runs applicable policies. Output:

```text
Policy

  ✓ no-friday-majors          (not applicable — weekday: tuesday)
  ✓ require-changelog         (allowed — changelog entry present)
  ⚠ warn:internal-major-bump  (Major bump on an internal package …)
  ✗ deny:external-cve         (axios ^1.7.7 vulnerable to CVE-2025-11110)

2 warnings, 1 violation.
```

Manual run:

```bash
versionx policy eval --plan plan.json
```

## Waivers

Every `deny` can be waived with mandatory expiry:

```toml
# .versionx/waivers.toml

[[waiver]]
rule = "external-cve"
match = { package = "axios", version = "^1.7.7" }
reason = "Upstream patch in progress. Tracking in INFRA-4421."
granted-by = "kody@honesttechservices.com"
granted-on = "2026-04-15"
expires = "2026-05-15"
```

- **Expiry is mandatory.** There is no way to express "waive forever" in the config.
- **Waivers are audited.** `versionx policy waivers` lists every active waiver with time remaining; `versionx policy waivers --expired` shows ones that have lapsed (which now deny the action).

## Fleet-wide policies

In the ops repo:

```toml
# acme/platform-ops/policies/baseline.policy.toml
[rule.supported-node]
when = { action = "install", runtime = "node" }
require = { semver-range = ">=20" }
decision = "deny"
reason = "Node 20+ required across the fleet."
```

Member repos inherit:

```toml
# versionx.toml
[policies]
inherit = ["fleet://acme-platform/baseline"]
```

Local repo policies can add but not remove fleet-inherited ones. Waivers for inherited rules still live in the local `waivers.toml` (so they're localized and auditable).

## Patterns

**Freeze windows:**

```toml
[rule.q4-release-freeze]
when = { action = "release" }
on   = { between = ["2026-12-10", "2026-12-31"] }
decision = "deny"
reason = "Q4 freeze. Emergency releases require a signed waiver."
```

**Require a specific approver:**

```toml
[rule.sensitive-package]
when = { action = "release", package = "billing" }
require = { pr-approver = ["kody@honesttechservices.com"] }
decision = "deny"
reason = "Billing releases require Kody's approval."
```

**Block majors unless waived:**

```toml
[rule.no-unreviewed-majors]
when = { action = "update", bump = "major" }
decision = "deny"
reason = "Major bumps require review. Add a waiver or downgrade to minor."
```

## Troubleshooting

- **Policy misses.** `versionx policy explain --rule <id>` walks the matching logic for the current plan.
- **Luau syntax error.** `versionx policy lint` runs every `.policy.lua` file in a dry sandbox.
- **Expired waiver in CI.** `versionx policy waivers --check` exits non-zero if any referenced waiver has expired.

## See also

- [`versionx.toml` reference](/reference/versionx-toml) — `[policies]` section.
- [`docs/spec/07-policy-engine.md`](https://github.com/KodyDennon/versionx/blob/main/docs/spec/07-policy-engine.md) — the authoritative spec.
