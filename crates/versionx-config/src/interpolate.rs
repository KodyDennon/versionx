//! Environment-variable interpolation for `versionx.toml` string values.
//!
//! Supports two syntaxes:
//!
//! - `${VAR}` — required; fails loudly if `VAR` is unset.
//! - `${VAR:-default}` — falls back to `default` if `VAR` is unset.
//!
//! Applied at config-load time. A future `@lazy` suffix will defer resolution
//! to adapter invocation; for 0.1.0 only eager mode is implemented.

use std::borrow::Cow;

use camino::Utf8Path;
use regex::Regex;

use crate::error::{ConfigError, ConfigResult};

/// Pattern: `${VAR}` or `${VAR:-fallback}`.
fn pattern() -> &'static Regex {
    use std::sync::OnceLock;
    static PAT: OnceLock<Regex> = OnceLock::new();
    PAT.get_or_init(|| {
        // Capture: 1 = VAR, 2 = optional fallback (without the `:-` prefix).
        Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}").expect("valid regex")
    })
}

/// Reader abstraction so tests can inject a fake env.
pub trait EnvReader {
    /// Return `Some(value)` if set, `None` if unset.
    fn get(&self, key: &str) -> Option<String>;
}

/// Default reader reading from the real process env.
#[derive(Debug, Default)]
pub struct ProcessEnv;

impl EnvReader for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Interpolate `${...}` patterns in `s`, using `env` to resolve names.
pub fn interpolate<'a>(
    s: &'a str,
    env: &dyn EnvReader,
    path: &Utf8Path,
) -> ConfigResult<Cow<'a, str>> {
    let pat = pattern();
    if !pat.is_match(s) {
        return Ok(Cow::Borrowed(s));
    }

    let mut out = String::with_capacity(s.len());
    let mut last = 0usize;
    for caps in pat.captures_iter(s) {
        let m = caps.get(0).unwrap();
        out.push_str(&s[last..m.start()]);

        let var_name = caps.get(1).unwrap().as_str();
        let fallback = caps.get(2).map(|m| m.as_str());
        let resolved = match env.get(var_name) {
            Some(v) => v,
            None => match fallback {
                Some(f) => f.to_string(),
                None => {
                    return Err(ConfigError::MissingEnv {
                        var: var_name.into(),
                        path: path.to_path_buf(),
                    });
                }
            },
        };
        out.push_str(&resolved);
        last = m.end();
    }
    out.push_str(&s[last..]);
    Ok(Cow::Owned(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct TestEnv(HashMap<String, String>);

    impl EnvReader for TestEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    fn env(pairs: &[(&str, &str)]) -> TestEnv {
        TestEnv(pairs.iter().map(|(k, v)| ((*k).to_string(), (*v).to_string())).collect())
    }

    #[test]
    fn leaves_plain_strings_untouched() {
        let env = env(&[]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("no-vars-here", &env, path).unwrap(), "no-vars-here");
    }

    #[test]
    fn substitutes_single_var() {
        let env = env(&[("FOO", "bar")]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("${FOO}", &env, path).unwrap(), "bar");
    }

    #[test]
    fn substitutes_multiple_vars() {
        let env = env(&[("A", "1"), ("B", "2")]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("${A}-${B}-${A}", &env, path).unwrap(), "1-2-1");
    }

    #[test]
    fn fallback_when_missing() {
        let env = env(&[]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("${UNSET:-default}", &env, path).unwrap(), "default");
    }

    #[test]
    fn fallback_ignored_when_set() {
        let env = env(&[("SET", "real")]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("${SET:-default}", &env, path).unwrap(), "real");
    }

    #[test]
    fn missing_required_var_errors() {
        let env = env(&[]);
        let path = Utf8Path::new("x.toml");
        let err = interpolate("${NOPE}", &env, path).unwrap_err();
        assert!(matches!(err, ConfigError::MissingEnv { .. }));
    }

    #[test]
    fn empty_fallback_is_valid() {
        let env = env(&[]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("a${X:-}b", &env, path).unwrap(), "ab");
    }

    #[test]
    fn dollar_outside_pattern_passes_through() {
        let env = env(&[]);
        let path = Utf8Path::new("x.toml");
        assert_eq!(interpolate("$dollar $1 $", &env, path).unwrap(), "$dollar $1 $");
    }
}
