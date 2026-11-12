use serde::{Deserialize, Serialize};
#[cfg(not(test))]
use std::path::PathBuf;
use std::{
    fs,
    io::{self, Read},
    path::Path,
};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FeedPreferences {
    pub hide_watched: bool,
    pub hide_shorts: bool,
    pub hide_lives: bool,
    pub newest_first: bool,
    pub channel: Option<String>,
}
impl FeedPreferences {
    pub fn include(&self, watched: bool, short: bool, live: bool, channel: &str) -> bool {
        !(self.hide_watched && watched)
            && !(self.hide_shorts && short)
            && !(self.hide_lives && live)
            && self
                .channel
                .as_deref()
                .is_none_or(|selected| selected == channel)
    }
}
#[cfg(not(test))]
pub fn path() -> io::Result<PathBuf> {
    let root = crate::history::history_path()?;
    Ok(root.with_file_name("feed-preferences.json"))
}
pub fn load(path: &Path) -> io::Result<FeedPreferences> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(FeedPreferences::default()),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "feed preferences too large",
        ));
    }
    let pref: FeedPreferences = serde_json::from_slice(&bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid feed preferences"))?;
    if pref.channel.as_ref().is_some_and(|c| c.len() > 4096) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid feed channel",
        ));
    }
    Ok(pref)
}
pub fn save(path: &Path, pref: &FeedPreferences) -> io::Result<()> {
    if pref.channel.as_ref().is_some_and(|c| c.len() > 4096) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid feed channel",
        ));
    }
    let bytes = serde_json::to_vec(pref)?;
    crate::history::write_atomic(path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_predicates_only_exclude_requested_attributes() {
        let pref = FeedPreferences {
            hide_watched: true,
            hide_shorts: true,
            hide_lives: true,
            newest_first: true,
            channel: Some("Canal".into()),
        };
        assert!(pref.include(false, false, false, "Canal"));
        assert!(!pref.include(true, false, false, "Canal"));
        assert!(!pref.include(false, true, false, "Canal"));
        assert!(!pref.include(false, false, true, "Canal"));
        assert!(!pref.include(false, false, false, "Outro"));
    }
}
