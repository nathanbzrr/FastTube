use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferSettings {
    pub max_mib: u32,
    pub back_mib: u32,
    pub cache_seconds: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BufferProfile {
    Small,
    #[default]
    Default,
    Large,
    Custom(BufferSettings),
}

impl BufferProfile {
    pub fn values(self) -> BufferSettings {
        match self {
            Self::Small => BufferSettings {
                max_mib: 32,
                back_mib: 8,
                cache_seconds: 15,
            },
            Self::Default => BufferSettings {
                max_mib: 64,
                back_mib: 16,
                cache_seconds: 30,
            },
            Self::Large => BufferSettings {
                max_mib: 128,
                back_mib: 32,
                cache_seconds: 60,
            },
            Self::Custom(values) => values,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        let values = self.values();
        if !(1..=1024).contains(&values.max_mib) {
            return Err("max_mib must be between 1 and 1024");
        }
        if values.back_mib > values.max_mib {
            return Err("back_mib must not exceed max_mib");
        }
        if !(1..=3600).contains(&values.cache_seconds) {
            return Err("cache_seconds must be between 1 and 3600");
        }
        Ok(())
    }
}

pub fn settings_path() -> io::Result<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "LOCALAPPDATA is not set"))?;
    let path = PathBuf::from(local);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LOCALAPPDATA must be absolute",
        ));
    }
    Ok(path.join("FastTube").join("buffer-settings.txt"))
}

pub fn load(path: &Path) -> io::Result<BufferProfile> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(BufferProfile::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid buffer settings");
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let fields: Vec<_> = text.split_whitespace().collect();
    let profile = match fields.as_slice() {
        ["1", "small"] => BufferProfile::Small,
        ["1", "default"] => BufferProfile::Default,
        ["1", "large"] => BufferProfile::Large,
        ["1", "custom", max, back, seconds] => BufferProfile::Custom(BufferSettings {
            max_mib: max.parse().map_err(|_| invalid())?,
            back_mib: back.parse().map_err(|_| invalid())?,
            cache_seconds: seconds.parse().map_err(|_| invalid())?,
        }),
        _ => return Err(invalid()),
    };
    profile
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidData, message))?;
    Ok(profile)
}

pub fn save(path: &Path, profile: BufferProfile) -> io::Result<()> {
    profile
        .validate()
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let content = match profile {
        BufferProfile::Small => "1 small\n".to_owned(),
        BufferProfile::Default => "1 default\n".to_owned(),
        BufferProfile::Large => "1 large\n".to_owned(),
        BufferProfile::Custom(values) => format!(
            "1 custom {} {} {}\n",
            values.max_mib, values.back_mib, values.cache_seconds
        ),
    };
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-buffer-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let result = (|| {
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Defaults for each newly opened video; quality is a preferred maximum height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackPreferences {
    pub speed_quarters: u8,
    pub max_height: Option<u32>,
}

impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            speed_quarters: 4,
            max_height: None,
        }
    }
}

impl PlaybackPreferences {
    fn valid(self) -> bool {
        (1..=8).contains(&self.speed_quarters)
            && matches!(
                self.max_height,
                None | Some(360 | 480 | 720 | 1080 | 1440 | 2160)
            )
    }

    pub fn format(self) -> String {
        let height = self.max_height.unwrap_or(1080);
        format!("bestvideo[height<={height}]+bestaudio/best[height<={height}]/best")
    }
}

pub fn playback_path() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("playback-settings.txt"))
}

pub fn load_playback(path: &Path) -> io::Result<PlaybackPreferences> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(PlaybackPreferences::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid playback settings");
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let fields: Vec<_> = text.split_whitespace().collect();
    let preferences = match fields.as_slice() {
        ["1", speed, height] => PlaybackPreferences {
            speed_quarters: speed.parse().map_err(|_| invalid())?,
            max_height: if *height == "auto" {
                None
            } else {
                Some(height.parse().map_err(|_| invalid())?)
            },
        },
        _ => return Err(invalid()),
    };
    if !preferences.valid() {
        return Err(invalid());
    }
    Ok(preferences)
}

pub fn save_playback(path: &Path, preferences: PlaybackPreferences) -> io::Result<()> {
    if !preferences.valid() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid playback settings",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".fasttube-playback-{}-{id}.tmp",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let height = preferences
        .max_height
        .map(|v| v.to_string())
        .unwrap_or_else(|| "auto".into());
    let result = (|| {
        writeln!(file, "1 {} {}", preferences.speed_quarters, height)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

pub fn theme_path() -> io::Result<PathBuf> {
    Ok(settings_path()?.with_file_name("theme-settings.txt"))
}

pub fn load_theme(path: &Path) -> io::Result<Theme> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Theme::default());
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1025).read_to_end(&mut bytes)?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid theme settings");
    if bytes.len() > 1024 {
        return Err(invalid());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
    let fields: Vec<_> = text.split_whitespace().collect();
    match fields.as_slice() {
        ["1", "dark"] => Ok(Theme::Dark),
        ["1", "light"] => Ok(Theme::Light),
        _ => Err(invalid()),
    }
}

pub fn save_theme(path: &Path, theme: Theme) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
        })?;
    fs::create_dir_all(parent)?;
    let (temporary, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".fasttube-theme-{}-{id}.tmp", std::process::id()));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let value = match theme {
        Theme::Dark => "dark",
        Theme::Light => "light",
    };
    let result = (|| {
        writeln!(file, "1 {value}")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path() -> PathBuf {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join(format!(
                "fasttube-settings-test-{}-{id}",
                std::process::id()
            ))
            .join("settings.txt")
    }

    #[test]
    fn theme_round_trip_default_and_corrupt_rejection() {
        let path = test_path();
        assert_eq!(load_theme(&path).unwrap(), Theme::Dark);
        for theme in [Theme::Light, Theme::Dark] {
            save_theme(&path, theme).unwrap();
            assert_eq!(load_theme(&path).unwrap(), theme);
        }
        assert_eq!(
            save_theme(Path::new(""), Theme::Light).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        for content in ["2 dark", "1 system", "1 dark extra", "1", ""] {
            fs::write(&path, content).unwrap();
            assert_eq!(
                load_theme(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
        fs::write(&path, vec![b'a'; 1025]).unwrap();
        assert_eq!(
            load_theme(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::write(&path, [0xff]).unwrap();
        assert_eq!(
            load_theme(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn playback_preferences_round_trip_validation_and_fallback_format() {
        let path = test_path();
        assert_eq!(
            load_playback(&path).unwrap(),
            PlaybackPreferences::default()
        );
        for speed_quarters in 1..=8 {
            for max_height in [
                None,
                Some(360),
                Some(480),
                Some(720),
                Some(1080),
                Some(1440),
                Some(2160),
            ] {
                let preferences = PlaybackPreferences {
                    speed_quarters,
                    max_height,
                };
                save_playback(&path, preferences).unwrap();
                assert_eq!(load_playback(&path).unwrap(), preferences);
                let height = max_height.unwrap_or(1080);
                assert_eq!(
                    preferences.format(),
                    format!("bestvideo[height<={height}]+bestaudio/best[height<={height}]/best")
                );
            }
        }
        let previous = fs::read(&path).unwrap();
        assert!(
            save_playback(
                &path,
                PlaybackPreferences {
                    speed_quarters: 0,
                    max_height: None
                }
            )
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), previous);
        for content in [
            "2 4 auto",
            "1 0 auto",
            "1 9 auto",
            "1 4 999",
            "1 4 auto extra",
            "",
        ] {
            fs::write(&path, content).unwrap();
            assert_eq!(
                load_playback(&path).unwrap_err().kind(),
                io::ErrorKind::InvalidData
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), content);
        }
        fs::write(&path, vec![b'a'; 1025]).unwrap();
        assert_eq!(
            load_playback(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn profiles_and_validation() {
        assert_eq!(
            BufferProfile::default().values(),
            BufferSettings {
                max_mib: 64,
                back_mib: 16,
                cache_seconds: 30
            }
        );
        for profile in [
            BufferProfile::Small,
            BufferProfile::Default,
            BufferProfile::Large,
        ] {
            assert!(profile.validate().is_ok());
        }
        for values in [
            BufferSettings {
                max_mib: 0,
                back_mib: 0,
                cache_seconds: 1,
            },
            BufferSettings {
                max_mib: 1025,
                back_mib: 0,
                cache_seconds: 1,
            },
            BufferSettings {
                max_mib: 1,
                back_mib: 2,
                cache_seconds: 1,
            },
            BufferSettings {
                max_mib: 1,
                back_mib: 0,
                cache_seconds: 0,
            },
            BufferSettings {
                max_mib: 1,
                back_mib: 0,
                cache_seconds: 3601,
            },
        ] {
            assert!(BufferProfile::Custom(values).validate().is_err());
        }
        assert!(
            BufferProfile::Custom(BufferSettings {
                max_mib: 1024,
                back_mib: 1024,
                cache_seconds: 3600
            })
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn round_trip_and_replace() {
        let path = test_path();
        assert_eq!(load(&path).unwrap(), BufferProfile::Default);
        for profile in [
            BufferProfile::Small,
            BufferProfile::Default,
            BufferProfile::Large,
            BufferProfile::Custom(BufferSettings {
                max_mib: 100,
                back_mib: 0,
                cache_seconds: 42,
            }),
        ] {
            save(&path, profile).unwrap();
            assert_eq!(load(&path).unwrap(), profile);
        }
        let invalid = BufferProfile::Custom(BufferSettings {
            max_mib: 0,
            back_mib: 0,
            cache_seconds: 1,
        });
        assert_eq!(
            save(&path, invalid).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(matches!(load(&path).unwrap(), BufferProfile::Custom(_)));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn rejects_corrupt_and_oversized_files() {
        let path = test_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for content in [
            "2 default",
            "1 custom 1 2 1",
            "1 custom -1 0 1",
            "1 default extra",
            "",
        ] {
            fs::write(&path, content).unwrap();
            assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
        fs::write(&path, vec![b'a'; 1025]).unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        fs::write(&path, [0xff]).unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
