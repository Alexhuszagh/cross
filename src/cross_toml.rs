#![doc = include_str!("../docs/cross_toml.md")]

use crate::errors::*;
use crate::{Target, TargetList};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeSet, HashMap};

/// Environment configuration
#[derive(Debug, Deserialize, PartialEq, Eq, Default)]
pub struct CrossEnvConfig {
    #[serde(default)]
    volumes: Vec<String>,
    #[serde(default)]
    passthrough: Vec<String>,
}

/// Build configuration
#[derive(Debug, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub struct CrossBuildConfig {
    #[serde(default)]
    env: CrossEnvConfig,
    xargo: Option<bool>,
    build_std: Option<bool>,
    default_target: Option<String>,
}

/// Target configuration
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct CrossTargetConfig {
    xargo: Option<bool>,
    build_std: Option<bool>,
    image: Option<String>,
    runner: Option<String>,
    #[serde(default)]
    env: CrossEnvConfig,
}

/// Cross configuration
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct CrossToml {
    #[serde(default, rename = "target")]
    pub targets: HashMap<Target, CrossTargetConfig>,
    #[serde(default)]
    pub build: CrossBuildConfig,
}

impl CrossToml {
    /// Parses the [`CrossToml`] from a string
    pub fn parse(toml_str: &str) -> Result<(Self, BTreeSet<String>)> {
        let mut tomld = toml::Deserializer::new(toml_str);
        Self::parse_from_deserializer(&mut tomld)
    }

    /// Parses the [`CrossToml`] from a string containing the Cargo.toml contents
    pub fn parse_from_cargo(cargo_toml_str: &str) -> Result<Option<(Self, BTreeSet<String>)>> {
        let cargo_toml: toml::Value = toml::from_str(cargo_toml_str)?;
        let cross_metadata_opt = cargo_toml
            .get("package")
            .and_then(|p| p.get("metadata"))
            .and_then(|m| m.get("cross"));

        if let Some(cross_meta) = cross_metadata_opt {
            Ok(Some(Self::parse_from_deserializer(cross_meta.clone())?))
        } else {
            Ok(None)
        }
    }

    /// Parses the [`CrossToml`] from a [`Deserializer`]
    fn parse_from_deserializer<'de, D>(deserializer: D) -> Result<(Self, BTreeSet<String>)>
    where
        D: Deserializer<'de>,
        D::Error: Send + Sync + 'static,
    {
        let mut unused = BTreeSet::new();
        let cfg = serde_ignored::deserialize(deserializer, |path| {
            unused.insert(path.to_string());
        })?;

        if !unused.is_empty() {
            eprintln!(
                "Warning: found unused key(s) in Cross configuration:\n > {}",
                unused.clone().into_iter().collect::<Vec<_>>().join(", ")
            );
        }

        Ok((cfg, unused))
    }

    /// Returns the `target.{}.image` part of `Cross.toml`
    pub fn image(&self, target: &Target) -> Option<String> {
        self.get_string(target, |t| &t.image)
    }

    /// Returns the `target.{}.runner` part of `Cross.toml`
    pub fn runner(&self, target: &Target) -> Option<String> {
        self.get_string(target, |t| &t.runner)
    }

    /// Returns the `build.xargo` or the `target.{}.xargo` part of `Cross.toml`
    pub fn xargo(&self, target: &Target) -> (Option<bool>, Option<bool>) {
        self.get_bool(target, |b| b.xargo, |t| t.xargo)
    }

    /// Returns the `build.build-std` or the `target.{}.build-std` part of `Cross.toml`
    pub fn build_std(&self, target: &Target) -> (Option<bool>, Option<bool>) {
        self.get_bool(target, |b| b.build_std, |t| t.build_std)
    }

    /// Returns the list of environment variables to pass through for `build`,
    pub fn env_passthrough_build(&self) -> &[String] {
        &self.build.env.passthrough
    }

    /// Returns the list of environment variables to pass through for `target`,
    pub fn env_passthrough_target(&self, target: &Target) -> &[String] {
        self.get_vec(target, |e| &e.passthrough)
    }

    /// Returns the list of environment variables to pass through for `build`,
    pub fn env_volumes_build(&self) -> &[String] {
        &self.build.env.volumes
    }

    /// Returns the list of environment variables to pass through for `target`,
    pub fn env_volumes_target(&self, target: &Target) -> &[String] {
        self.get_vec(target, |e| &e.volumes)
    }

    /// Returns the default target to build,
    pub fn default_target(&self, target_list: &TargetList) -> Option<Target> {
        self.build
            .default_target
            .as_ref()
            .map(|t| Target::from(t, target_list))
    }

    /// Returns a reference to the [`CrossTargetConfig`] of a specific `target`
    fn get_target(&self, target: &Target) -> Option<&CrossTargetConfig> {
        self.targets.get(target)
    }

    fn get_string(
        &self,
        target: &Target,
        get: impl Fn(&CrossTargetConfig) -> &Option<String>,
    ) -> Option<String> {
        self.get_target(target).and_then(|t| get(t).clone())
    }

    fn get_bool(
        &self,
        target: &Target,
        get_build: impl Fn(&CrossBuildConfig) -> Option<bool>,
        get_target: impl Fn(&CrossTargetConfig) -> Option<bool>,
    ) -> (Option<bool>, Option<bool>) {
        let build = get_build(&self.build);
        let target = self.get_target(target).and_then(get_target);

        (build, target)
    }

    fn get_vec(&self, target: &Target, get: impl Fn(&CrossEnvConfig) -> &[String]) -> &[String] {
        self.get_target(target).map_or(&[], |t| get(&t.env))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn parse_empty_toml() -> Result<()> {
        let cfg = CrossToml {
            targets: HashMap::new(),
            build: CrossBuildConfig::default(),
        };
        let (parsed_cfg, unused) = CrossToml::parse("")?;

        assert_eq!(parsed_cfg, cfg);
        assert!(unused.is_empty());

        Ok(())
    }

    #[test]
    pub fn parse_build_toml() -> Result<()> {
        let cfg = CrossToml {
            targets: HashMap::new(),
            build: CrossBuildConfig {
                env: CrossEnvConfig {
                    volumes: vec!["VOL1_ARG".to_string(), "VOL2_ARG".to_string()],
                    passthrough: vec!["VAR1".to_string(), "VAR2".to_string()],
                },
                xargo: Some(true),
                build_std: None,
                default_target: None,
            },
        };

        let test_str = r#"
          [build]
          xargo = true

          [build.env]
          volumes = ["VOL1_ARG", "VOL2_ARG"]
          passthrough = ["VAR1", "VAR2"]
        "#;
        let (parsed_cfg, unused) = CrossToml::parse(test_str)?;

        assert_eq!(parsed_cfg, cfg);
        assert!(unused.is_empty());

        Ok(())
    }

    #[test]
    pub fn parse_target_toml() -> Result<()> {
        let mut target_map = HashMap::new();
        target_map.insert(
            Target::BuiltIn {
                triple: "aarch64-unknown-linux-gnu".to_string(),
            },
            CrossTargetConfig {
                env: CrossEnvConfig {
                    passthrough: vec!["VAR1".to_string(), "VAR2".to_string()],
                    volumes: vec!["VOL1_ARG".to_string(), "VOL2_ARG".to_string()],
                },
                xargo: Some(false),
                build_std: Some(true),
                image: Some("test-image".to_string()),
                runner: None,
            },
        );

        let cfg = CrossToml {
            targets: target_map,
            build: CrossBuildConfig::default(),
        };

        let test_str = r#"
            [target.aarch64-unknown-linux-gnu.env]
            volumes = ["VOL1_ARG", "VOL2_ARG"]
            passthrough = ["VAR1", "VAR2"]
            [target.aarch64-unknown-linux-gnu]
            xargo = false
            build-std = true
            image = "test-image"
        "#;
        let (parsed_cfg, unused) = CrossToml::parse(test_str)?;

        assert_eq!(parsed_cfg, cfg);
        assert!(unused.is_empty());

        Ok(())
    }

    #[test]
    pub fn parse_from_empty_cargo_toml() -> Result<()> {
        let test_str = r#"
          [package]
          name = "cargo_toml_test_package"
          version = "0.1.0"

          [dependencies]
          cross = "1.2.3"
        "#;

        let res = CrossToml::parse_from_cargo(test_str)?;
        assert!(res.is_none());

        Ok(())
    }

    #[test]
    pub fn parse_from_cargo_toml() -> Result<()> {
        let cfg = CrossToml {
            targets: HashMap::new(),
            build: CrossBuildConfig {
                env: CrossEnvConfig {
                    passthrough: vec![],
                    volumes: vec![],
                },
                build_std: None,
                xargo: Some(true),
                default_target: None,
            },
        };

        let test_str = r#"
          [package]
          name = "cargo_toml_test_package"
          version = "0.1.0"

          [dependencies]
          cross = "1.2.3"

          [package.metadata.cross.build]
          xargo = true
        "#;

        if let Some((parsed_cfg, _unused)) = CrossToml::parse_from_cargo(test_str)? {
            assert_eq!(parsed_cfg, cfg);
        } else {
            panic!("Parsing result is None");
        }

        Ok(())
    }
}
