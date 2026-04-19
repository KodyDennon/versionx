//! Cross-ecosystem version translation.
//!
//! Different package registries have slightly different canonical
//! forms:
//!   - **SemVer** (npm, crates.io): `1.2.3-rc.1`, `1.2.3+build.5`.
//!   - **PEP 440** (PyPI): `1.2.3rc1`, `1.2.3+build5`.
//!   - **RubyGems**: `1.2.3.rc.1` (periods instead of hyphens).
//!
//! We pick SemVer as the canonical form and translate to + from the
//! other two. Any conversion that loses information (e.g. PEP 440's
//! epochs or post-releases) produces a [`Warning`] the caller can
//! surface — the conversion still succeeds, lossy aspects are just
//! flagged.

use std::fmt;

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Translated {
    pub output: String,
    pub warnings: Vec<Warning>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Warning {
    pub message: String,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Ecosystem {
    Semver,
    Pep440,
    Rubygems,
}

impl fmt::Display for Ecosystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Semver => "semver",
            Self::Pep440 => "pep440",
            Self::Rubygems => "rubygems",
        })
    }
}

/// Translate a canonical SemVer version string into `target`'s
/// preferred form. Returns the translated string + any warnings about
/// information loss.
pub fn from_semver(input: &str, target: Ecosystem) -> Translated {
    let mut warnings = Vec::new();
    let parsed = semver::Version::parse(input);
    let Ok(v) = parsed else {
        warnings.push(Warning { message: format!("not a valid SemVer: {input}") });
        return Translated { output: input.to_string(), warnings };
    };
    match target {
        Ecosystem::Semver => Translated { output: v.to_string(), warnings },
        Ecosystem::Pep440 => {
            let mut out = format!("{}.{}.{}", v.major, v.minor, v.patch);
            if !v.pre.is_empty() {
                // PEP 440 pre-releases: aN/bN/rcN with no dots.
                out.push_str(&semver_pre_to_pep440(&v.pre.as_str().to_string(), &mut warnings));
            }
            if !v.build.is_empty() {
                // PEP 440 local segment uses `+tag` with alnum only.
                let sanitized = v.build.as_str().replace('.', "");
                out.push('+');
                out.push_str(&sanitized);
            }
            Translated { output: out, warnings }
        }
        Ecosystem::Rubygems => {
            // RubyGems wants `.rc.N` instead of `-rc.N`.
            let mut out = format!("{}.{}.{}", v.major, v.minor, v.patch);
            if !v.pre.is_empty() {
                out.push('.');
                out.push_str(v.pre.as_str());
            }
            if !v.build.is_empty() {
                warnings.push(Warning {
                    message: format!("RubyGems ignores build metadata: +{}", v.build),
                });
            }
            Translated { output: out, warnings }
        }
    }
}

/// Parse back into canonical SemVer from an ecosystem-specific form.
/// Lossy in some directions (PEP 440 post-releases become build
/// metadata; RubyGems `.rc.N` becomes `-rc.N`).
pub fn into_semver(input: &str, source: Ecosystem) -> Translated {
    let mut warnings = Vec::new();
    match source {
        Ecosystem::Semver => {
            let normalized = semver::Version::parse(input)
                .map(|v| v.to_string())
                .unwrap_or_else(|_| input.to_string());
            Translated { output: normalized, warnings }
        }
        Ecosystem::Pep440 => {
            let converted = pep440_to_semver(input, &mut warnings);
            Translated { output: converted, warnings }
        }
        Ecosystem::Rubygems => {
            // `1.2.3.rc.1` → `1.2.3-rc.1`.
            let (core, tail) = split_on_first_prerelease(input);
            let out = if let Some(tail) = tail {
                format!("{core}-{}", tail.trim_start_matches('.'))
            } else {
                core.to_string()
            };
            Translated { output: out, warnings }
        }
    }
}

fn semver_pre_to_pep440(pre: &str, warnings: &mut Vec<Warning>) -> String {
    // Very common shapes: `rc.N`, `alpha.N`, `beta.N`, `pre.N`.
    // PEP 440 maps to `rcN`, `aN`, `bN`, `rcN` respectively.
    let mut parts = pre.splitn(2, '.');
    let tag = parts.next().unwrap_or("").to_ascii_lowercase();
    let num = parts.next().unwrap_or("0");
    let pep_tag = match tag.as_str() {
        "alpha" | "a" => "a",
        "beta" | "b" => "b",
        "rc" | "pre" => "rc",
        other => {
            warnings.push(Warning {
                message: format!("unknown pre-release tag `{other}`, passing through"),
            });
            return format!(".{pre}");
        }
    };
    format!("{pep_tag}{num}")
}

fn pep440_to_semver(input: &str, warnings: &mut Vec<Warning>) -> String {
    // Minimal PEP 440 parser: N.N.N[aN|bN|rcN][.postN][.devN][+local].
    // We handle the common 80% — epochs + dev + post are all noted as
    // warnings when present.
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"^(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)(?P<pre>(?:a|b|c|rc|alpha|beta|pre)\d+)?(?:\.post(?P<post>\d+))?(?:\.dev(?P<dev>\d+))?(?:\+(?P<local>[A-Za-z0-9.]+))?$",
        )
        .expect("static regex")
    });
    let Some(caps) = re.captures(input) else {
        warnings.push(Warning { message: format!("unrecognized PEP 440 form: {input}") });
        return input.to_string();
    };
    let mut out = format!("{}.{}.{}", &caps["major"], &caps["minor"], &caps["patch"]);
    if let Some(pre) = caps.name("pre") {
        let s = pre.as_str();
        let (tag, num) = s.trim_start_matches('.').split_at(tag_split(s));
        let semver_tag = match tag {
            "a" | "alpha" => "alpha",
            "b" | "beta" => "beta",
            "rc" | "c" | "pre" => "rc",
            other => {
                warnings.push(Warning { message: format!("unrecognized PEP 440 tag `{other}`") });
                other
            }
        };
        out.push_str(&format!("-{semver_tag}.{num}"));
    }
    if let Some(post) = caps.name("post") {
        warnings.push(Warning {
            message: format!("PEP 440 post-release .post{} folded into build", post.as_str()),
        });
        out.push_str(&format!("+post.{}", post.as_str()));
    }
    if let Some(dev) = caps.name("dev") {
        warnings.push(Warning {
            message: format!("PEP 440 dev-release .dev{} folded into build", dev.as_str()),
        });
        out.push_str(&format!("+dev.{}", dev.as_str()));
    }
    if let Some(local) = caps.name("local") {
        if out.contains('+') {
            out.push('.');
        } else {
            out.push('+');
        }
        out.push_str(local.as_str());
    }
    out
}

fn tag_split(s: &str) -> usize {
    s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len())
}

fn split_on_first_prerelease(input: &str) -> (String, Option<String>) {
    // RubyGems uses `.rc.N` / `.pre.N`; we look for the first
    // alphabetic segment.
    let mut parts = input.split('.').collect::<Vec<_>>();
    for i in 0..parts.len() {
        if parts[i].chars().any(|c| c.is_alphabetic()) {
            let core = parts[..i].join(".");
            let tail = parts.split_off(i).join(".");
            return (core, Some(tail));
        }
    }
    (input.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_to_pep440_rc() {
        let out = from_semver("1.2.3-rc.1", Ecosystem::Pep440);
        assert_eq!(out.output, "1.2.3rc1");
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn semver_to_rubygems_rc() {
        let out = from_semver("1.2.3-rc.1", Ecosystem::Rubygems);
        assert_eq!(out.output, "1.2.3.rc.1");
    }

    #[test]
    fn pep440_to_semver_rc() {
        let out = into_semver("1.2.3rc1", Ecosystem::Pep440);
        assert_eq!(out.output, "1.2.3-rc.1");
    }

    #[test]
    fn pep440_post_is_lossy() {
        let out = into_semver("1.2.3.post5", Ecosystem::Pep440);
        assert!(out.output.contains("+post.5"));
        assert!(!out.warnings.is_empty());
    }

    #[test]
    fn rubygems_round_trip() {
        let sv = from_semver("1.2.3-alpha.2", Ecosystem::Rubygems);
        let back = into_semver(&sv.output, Ecosystem::Rubygems);
        assert_eq!(back.output, "1.2.3-alpha.2");
    }

    #[test]
    fn rubygems_drops_build_metadata() {
        let out = from_semver("1.2.3+abc.def", Ecosystem::Rubygems);
        assert_eq!(out.output, "1.2.3");
        assert_eq!(out.warnings.len(), 1);
    }
}
