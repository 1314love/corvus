//! The single per-user Corvus home.
//!
//! Every long-lived file the daemon owns lives under one root: the recording
//! database in data/, logs in logs/, configuration in config/, copied binaries
//! in bin/ and the unpacked extension in extension/. The root is chosen
//! explicitly with --home, then by CORVUS_HOME, then by the platform data
//! directory.

use std::io;
use std::path::{Path, PathBuf};

/// Environment variable naming the Corvus home.
pub const HOME_ENV: &str = "CORVUS_HOME";

/// A resolved Corvus home root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HomeLayout {
    root: PathBuf,
}

impl HomeLayout {
    /// Resolve the home root. An explicit path wins over a non-empty
    /// CORVUS_HOME; both win over the platform default. Relative paths are
    /// made absolute against the current directory and not canonicalized.
    pub fn resolve(explicit: Option<PathBuf>) -> io::Result<Self> {
        resolve(explicit)
    }

    /// The home root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where release binaries are installed.
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Where the recording database lives.
    pub fn data_dir(&self) -> PathBuf {
        self.root.join("data")
    }

    /// Where rolling daemon logs live.
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Where user configuration lives.
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }

    /// Where the unpacked extension lives.
    pub fn extension_dir(&self) -> PathBuf {
        self.root.join("extension")
    }

    /// The recording database path.
    pub fn db_path(&self) -> PathBuf {
        self.data_dir().join("corvus.db")
    }

    /// The daemon log prefix used for daily rotation.
    pub fn log_path(&self) -> PathBuf {
        self.logs_dir().join("corvusd.log")
    }

    /// The service stdout log path.
    pub fn stdout_log_path(&self) -> PathBuf {
        self.logs_dir().join("corvusd.log")
    }

    /// The service stderr log path.
    pub fn stderr_log_path(&self) -> PathBuf {
        self.logs_dir().join("corvusd.err.log")
    }

    /// Create the root and its data, logs and config directories. The bin and
    /// extension directories are owned by the installer and are not created
    /// here.
    pub fn ensure(&self) -> io::Result<()> {
        for directory in [
            self.root(),
            self.data_dir().as_path(),
            self.logs_dir().as_path(),
            self.config_dir().as_path(),
        ] {
            std::fs::create_dir_all(directory)?;
        }
        Ok(())
    }
}

/// Resolve the home and the recording database path together.
///
/// An explicit database path always wins. When no database path is given, a
/// home supplied with --home or CORVUS_HOME owns the database. Otherwise
/// CORVUS_DB_PATH and then the platform default home decide the path.
pub fn resolve_database(
    explicit_db: Option<PathBuf>,
    explicit_home: Option<PathBuf>,
) -> io::Result<(HomeLayout, PathBuf)> {
    let configured_home = explicit_home
        .as_ref()
        .is_some_and(|path| !path.as_os_str().is_empty())
        || configured_by_env();
    let layout = HomeLayout::resolve(explicit_home)?;
    let path = explicit_db.unwrap_or_else(|| {
        if configured_home {
            layout.db_path()
        } else {
            corvus_record::resolve_path(None)
        }
    });
    Ok((layout, path))
}

/// Resolve the home root, with the same precedence as HomeLayout::resolve.
pub fn resolve(explicit: Option<PathBuf>) -> io::Result<HomeLayout> {
    let root = explicit
        .filter(|path| !path.as_os_str().is_empty())
        .or_else(home_env)
        .or_else(platform_default)
        .unwrap_or_else(|| PathBuf::from(".corvus"));
    Ok(HomeLayout {
        root: absolute(root)?,
    })
}

/// The non-empty CORVUS_HOME value, when configured.
pub fn home_env() -> Option<PathBuf> {
    std::env::var_os(HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Whether the home came from CORVUS_HOME (it may still be relative).
pub fn configured_by_env() -> bool {
    home_env().is_some()
}

fn platform_default() -> Option<PathBuf> {
    if cfg!(windows) {
        return non_empty_var("LOCALAPPDATA")
            .map(|path| path.join("Corvus"))
            .or_else(|| non_empty_var("USERPROFILE").map(|path| path.join(".corvus")));
    }
    if cfg!(target_os = "macos") {
        return non_empty_var("HOME").map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("Corvus")
        });
    }
    non_empty_var("XDG_DATA_HOME")
        .map(|path| path.join("corvus"))
        .or_else(|| {
            non_empty_var("HOME").map(|path| path.join(".local").join("share").join("corvus"))
        })
}

fn non_empty_var(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn absolute(path: PathBuf) -> io::Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }
    Ok(std::env::current_dir()?.join(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        previous: Option<OsString>,
    }

    impl EnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var_os(HOME_ENV);
            std::env::set_var(HOME_ENV, value);
            Self { previous }
        }

        fn remove() -> Self {
            let previous = std::env::var_os(HOME_ENV);
            std::env::remove_var(HOME_ENV);
            Self { previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(HOME_ENV, value),
                None => std::env::remove_var(HOME_ENV),
            }
        }
    }

    struct DbEnvGuard {
        previous: Option<OsString>,
    }

    impl DbEnvGuard {
        fn set(value: &str) -> Self {
            let previous = std::env::var_os("CORVUS_DB_PATH");
            std::env::set_var("CORVUS_DB_PATH", value);
            Self { previous }
        }
    }

    impl Drop for DbEnvGuard {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("CORVUS_DB_PATH", value),
                None => std::env::remove_var("CORVUS_DB_PATH"),
            }
        }
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn unique(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("corvus-home-{}-{stamp}-{name}", std::process::id()))
    }

    #[test]
    fn explicit_root_wins_and_relative_roots_become_absolute() {
        let _env_lock = lock_env();
        let _env = EnvGuard::set("env-home-is-ignored");
        let layout = HomeLayout::resolve(Some(PathBuf::from("relative-corvus-home"))).unwrap();
        assert!(layout.root().is_absolute());
        assert!(layout.root().ends_with("relative-corvus-home"));
        assert!(!layout.root().ends_with("env-home-is-ignored"));
    }

    #[test]
    fn env_root_is_used_when_no_explicit_root_is_given() {
        let _env_lock = lock_env();
        let _env = EnvGuard::set("relative-env-corvus-home");
        let layout = resolve(None).unwrap();
        assert!(layout.root().is_absolute());
        assert!(layout.root().ends_with("relative-env-corvus-home"));
    }

    #[test]
    fn empty_env_is_ignored() {
        let _env_lock = lock_env();
        let _env = EnvGuard::set("");
        assert_eq!(home_env(), None);
        assert!(!configured_by_env());
        let layout = resolve(None).unwrap();
        assert!(layout.root().is_absolute());
    }

    #[test]
    fn default_layout_has_one_root_for_every_path() {
        let _env_lock = lock_env();
        let _env = EnvGuard::remove();
        let layout = resolve(None).unwrap();
        assert!(layout.root().is_absolute());
        assert_eq!(layout.bin_dir(), layout.root().join("bin"));
        assert_eq!(layout.data_dir(), layout.root().join("data"));
        assert_eq!(layout.logs_dir(), layout.root().join("logs"));
        assert_eq!(layout.config_dir(), layout.root().join("config"));
        assert_eq!(layout.extension_dir(), layout.root().join("extension"));
        assert_eq!(layout.db_path(), layout.data_dir().join("corvus.db"));
        assert_eq!(layout.log_path(), layout.logs_dir().join("corvusd.log"));
        assert_eq!(
            layout.stdout_log_path(),
            layout.logs_dir().join("corvusd.log")
        );
        assert_eq!(
            layout.stderr_log_path(),
            layout.logs_dir().join("corvusd.err.log")
        );
    }

    #[test]
    fn database_precedence_is_explicit_then_home_then_environment() {
        let _env_lock = lock_env();
        let _home = EnvGuard::remove();
        let _db_env = DbEnvGuard::set("environment-corvus.db");
        let (_, from_env) = resolve_database(None, None).unwrap();
        assert_eq!(from_env, PathBuf::from("environment-corvus.db"));

        let (_, explicit) =
            resolve_database(Some(PathBuf::from("explicit-corvus.db")), None).unwrap();
        assert_eq!(explicit, PathBuf::from("explicit-corvus.db"));

        let (layout, from_home) =
            resolve_database(None, Some(PathBuf::from("configured-home"))).unwrap();
        assert_eq!(from_home, layout.db_path());
        assert!(from_home.ends_with("configured-home/data/corvus.db"));
    }

    #[test]
    fn ensure_creates_data_logs_and_config_but_not_install_directories() {
        let root = unique("ensure");
        let layout = HomeLayout::resolve(Some(root.clone())).unwrap();
        layout.ensure().unwrap();
        assert!(layout.root().is_dir());
        assert!(layout.data_dir().is_dir());
        assert!(layout.logs_dir().is_dir());
        assert!(layout.config_dir().is_dir());
        assert!(!layout.bin_dir().exists());
        assert!(!layout.extension_dir().exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
