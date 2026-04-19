//! Load a `versionx.toml` from disk: parse, interpolate, validate.

use std::collections::HashMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::error::{ConfigError, ConfigResult};
use crate::interpolate::{EnvReader, ProcessEnv, interpolate};
use crate::schema::{SUPPORTED_SCHEMA_VERSION, VersionxConfig};

/// The full load result: the parsed config plus the absolute path it came from
/// (or `None` if synthesized in memory).
#[derive(Clone, Debug)]
pub struct EffectiveConfig {
    /// Path to the `versionx.toml` that produced this config, if any.
    pub path: Option<Utf8PathBuf>,
    /// The parsed + interpolated + validated config.
    pub config: VersionxConfig,
    /// Source label: `"file"`, `"synthesized"`, `"defaults"`.
    pub source: ConfigSource,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    /// Read from a file on disk.
    File,
    /// Synthesized in memory from filesystem signals (zero-config path).
    Synthesized,
    /// All defaults. Used when load is called with no config present and
    /// detection isn't wanted.
    Defaults,
}

/// Load a config from the given path. Applies env-var interpolation against
/// the process environment.
pub fn load(path: impl AsRef<Utf8Path>) -> ConfigResult<EffectiveConfig> {
    load_with_env(path, &ProcessEnv)
}

/// Load a config from a TOML string (no file I/O).
pub fn load_from_str(source: &str, env: &dyn EnvReader) -> ConfigResult<VersionxConfig> {
    let interp = interpolate(source, env, Utf8Path::new("<string>"))?;
    parse_and_validate(&interp, Utf8Path::new("<string>"))
}

/// Load with an injectable env reader (mainly for tests).
pub fn load_with_env(
    path: impl AsRef<Utf8Path>,
    env: &dyn EnvReader,
) -> ConfigResult<EffectiveConfig> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            ConfigError::NotFound { path: path.to_path_buf() }
        } else {
            ConfigError::Io { path: path.to_path_buf(), source }
        }
    })?;

    let interpolated = interpolate(&raw, env, path)?;
    let config = parse_and_validate(&interpolated, path)?;

    Ok(EffectiveConfig { path: Some(path.to_path_buf()), config, source: ConfigSource::File })
}

/// Load a `.env`-style file into a plain `HashMap`. Missing file returns
/// an empty map (not an error) — `.env` is optional.
///
/// Does **not** modify the process environment. Callers overlay the returned
/// map onto their env reader as needed.
#[allow(dead_code)] // Wired up by versionx-core::commands::sync (landing in 0.1.0).
pub fn load_dotenv(path: impl AsRef<Utf8Path>) -> HashMap<String, String> {
    let path = path.as_ref();
    let Ok(contents) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    parse_dotenv(&contents)
}

#[allow(dead_code)] // Same as `load_dotenv`.
pub fn parse_dotenv(contents: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Support optional `export` prefix for shell-script compat.
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        // Strip matching outer quotes.
        let value = if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        out.insert(key.to_string(), value.to_string());
    }
    out
}

fn parse_and_validate(source: &str, path: &Utf8Path) -> ConfigResult<VersionxConfig> {
    let config: VersionxConfig = toml::from_str(source).map_err(|e| {
        // toml::de::Error's Display is structured enough for humans; we
        // surface it verbatim plus the file path.
        ConfigError::TomlParse { path: path.to_path_buf(), source: e }
    })?;

    validate(&config, path)?;
    Ok(config)
}

/// Run cheap semantic checks on a parsed config.
fn validate(config: &VersionxConfig, path: &Utf8Path) -> ConfigResult<()> {
    if let Some(sv) = &config.versionx.schema_version
        && sv.as_str() > SUPPORTED_SCHEMA_VERSION
    {
        return Err(ConfigError::SchemaTooNew {
            path: path.to_path_buf(),
            found: sv.clone(),
            supported: SUPPORTED_SCHEMA_VERSION.into(),
        });
    }

    if let Some(release) = &config.release {
        let valid_strategies = ["pr-title", "conventional", "changesets", "manual"];
        if !valid_strategies.contains(&release.strategy.as_str()) {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                message: format!(
                    "[release] strategy = {:?} is not one of {:?}",
                    release.strategy, valid_strategies
                ),
            });
        }
        let valid_ai = ["mcp", "byo-api", "off"];
        if !valid_ai.contains(&release.ai_assist.as_str()) {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                message: format!(
                    "[release] ai_assist = {:?} is not one of {:?}",
                    release.ai_assist, valid_ai
                ),
            });
        }
    }

    for (name, link) in &config.links {
        let valid = ["submodule", "subtree", "virtual", "ref"];
        if !valid.contains(&link.kind.as_str()) {
            return Err(ConfigError::Invalid {
                path: path.to_path_buf(),
                message: format!("[links.{name}] type = {:?} is not one of {:?}", link.kind, valid),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpolate::EnvReader;
    use std::collections::HashMap;

    struct MapEnv(HashMap<String, String>);
    impl EnvReader for MapEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn minimal_config_parses() {
        let src = r#"
            [runtimes]
            node = "22.12.0"
            python = "3.12"
        "#;
        let env = MapEnv(HashMap::new());
        let cfg = load_from_str(src, &env).unwrap();
        assert_eq!(cfg.runtimes.tools.get("node").unwrap().version(), "22.12.0");
        assert_eq!(cfg.runtimes.tools.get("python").unwrap().version(), "3.12");
    }

    #[test]
    fn release_defaults_applied() {
        let src = r"
            [release]
        ";
        let env = MapEnv(HashMap::new());
        let cfg = load_from_str(src, &env).unwrap();
        let rel = cfg.release.unwrap();
        assert_eq!(rel.strategy, "pr-title");
        assert_eq!(rel.ai_assist, "mcp");
        assert_eq!(rel.plan_ttl, "24h");
    }

    #[test]
    fn bad_strategy_rejected() {
        let src = r#"
            [release]
            strategy = "what-is-this"
        "#;
        let env = MapEnv(HashMap::new());
        let err = load_from_str(src, &env).unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn env_interpolation_works() {
        let src = r#"
            [vars]
            TOKEN = "${TOKEN:-fallback}"
        "#;
        let env = MapEnv(HashMap::new());
        let cfg = load_from_str(src, &env).unwrap();
        assert_eq!(cfg.vars["TOKEN"], "fallback");
    }

    #[test]
    fn dotenv_parses_typical_shapes() {
        let input = r#"
# comment line
FOO=bar
BAZ="quoted value"
 QUX ='single'
export EXPORTED=hi
MALFORMED
=garbage
        "#;
        let env = parse_dotenv(input);
        assert_eq!(env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(env.get("BAZ"), Some(&"quoted value".to_string()));
        assert_eq!(env.get("QUX"), Some(&"single".to_string()));
        assert_eq!(env.get("EXPORTED"), Some(&"hi".to_string()));
        assert!(!env.contains_key("MALFORMED"));
    }

    #[test]
    fn unknown_toplevel_key_rejected() {
        let src = r"
            [what-is-this]
            x = 1
        ";
        let env = MapEnv(HashMap::new());
        let err = load_from_str(src, &env).unwrap_err();
        // serde's `deny_unknown_fields` surfaces via TomlParse.
        assert!(matches!(err, ConfigError::TomlParse { .. }), "got {err:?}");
    }
}
