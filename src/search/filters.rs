use super::{SearchResult, SearchResultKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchSort {
    #[default]
    Relevance,
    Date,
    Views,
    Rating,
}

impl SearchSort {
    #[allow(dead_code)]
    pub fn label(self, language: crate::i18n::UiLanguage) -> &'static str {
        match self {
            Self::Relevance => language.pick("Relevância", "Relevance"),
            Self::Date => language.pick("Data de envio", "Upload date"),
            Self::Views => language.pick("Popularidade", "View count"),
            Self::Rating => language.pick("Avaliação", "Rating"),
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchDurationFilter {
    #[default]
    Any,
    Short,
    Medium,
    Long,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchTypeFilter {
    #[default]
    All,
    Videos,
    Shorts,
    Channels,
    Playlists,
    Movies,
    Live,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchUploadDateFilter {
    #[default]
    Any,
    Today,
    ThisWeek,
    ThisMonth,
    ThisYear,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchFeatureFilters {
    pub live: bool,
    pub four_k: bool,
    pub hd: bool,
    pub subtitles: bool,
    pub creative_commons: bool,
    pub three_sixty: bool,
    pub vr180: bool,
    pub three_d: bool,
    pub hdr: bool,
    pub location: bool,
    pub purchased: bool,
}

impl SearchFeatureFilters {
    #[allow(dead_code)]
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }

    #[allow(dead_code)]
    pub fn has(self, feature: crate::innertube::SearchFeature) -> bool {
        match feature {
            crate::innertube::SearchFeature::Live => self.live,
            crate::innertube::SearchFeature::FourK => self.four_k,
            crate::innertube::SearchFeature::Hd => self.hd,
            crate::innertube::SearchFeature::Subtitles => self.subtitles,
            crate::innertube::SearchFeature::CreativeCommons => self.creative_commons,
            crate::innertube::SearchFeature::ThreeSixty => self.three_sixty,
            crate::innertube::SearchFeature::Vr180 => self.vr180,
            crate::innertube::SearchFeature::ThreeD => self.three_d,
            crate::innertube::SearchFeature::Hdr => self.hdr,
            crate::innertube::SearchFeature::Location => self.location,
            crate::innertube::SearchFeature::Purchased => self.purchased,
        }
    }

    #[allow(dead_code)]
    pub fn toggle(&mut self, feature: crate::innertube::SearchFeature) {
        let slot = match feature {
            crate::innertube::SearchFeature::Live => &mut self.live,
            crate::innertube::SearchFeature::FourK => &mut self.four_k,
            crate::innertube::SearchFeature::Hd => &mut self.hd,
            crate::innertube::SearchFeature::Subtitles => &mut self.subtitles,
            crate::innertube::SearchFeature::CreativeCommons => &mut self.creative_commons,
            crate::innertube::SearchFeature::ThreeSixty => &mut self.three_sixty,
            crate::innertube::SearchFeature::Vr180 => &mut self.vr180,
            crate::innertube::SearchFeature::ThreeD => &mut self.three_d,
            crate::innertube::SearchFeature::Hdr => &mut self.hdr,
            crate::innertube::SearchFeature::Location => &mut self.location,
            crate::innertube::SearchFeature::Purchased => &mut self.purchased,
        };
        *slot = !*slot;
    }

    #[allow(dead_code)]
    pub fn to_innertube_features(self) -> Vec<crate::innertube::SearchFeature> {
        let mut out = Vec::new();
        for feature in [
            crate::innertube::SearchFeature::Live,
            crate::innertube::SearchFeature::FourK,
            crate::innertube::SearchFeature::Hd,
            crate::innertube::SearchFeature::Subtitles,
            crate::innertube::SearchFeature::CreativeCommons,
            crate::innertube::SearchFeature::ThreeSixty,
            crate::innertube::SearchFeature::Vr180,
            crate::innertube::SearchFeature::ThreeD,
            crate::innertube::SearchFeature::Hdr,
            crate::innertube::SearchFeature::Location,
            crate::innertube::SearchFeature::Purchased,
        ] {
            if self.has(feature) {
                out.push(feature);
            }
        }
        out
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilters {
    pub sort: SearchSort,
    pub upload_date: crate::innertube::SearchUploadDate,
    pub result_type: crate::innertube::SearchResultType,
    pub duration: crate::innertube::SearchDuration,
    pub features: Vec<crate::innertube::SearchFeature>,
    pub kind: SearchTypeFilter,
}

impl SearchFilters {
    pub fn is_default(&self) -> bool {
        self.sort == SearchSort::default()
            && self.upload_date == crate::innertube::SearchUploadDate::Any
            && self.result_type == crate::innertube::SearchResultType::Any
            && self.duration == crate::innertube::SearchDuration::Any
            && self.features.is_empty()
            && self.kind == SearchTypeFilter::default()
    }

    pub fn to_innertube(&self) -> crate::innertube::SearchFilters {
        let mut features = self.features.clone();
        if self.kind == SearchTypeFilter::Live
            && !features.contains(&crate::innertube::SearchFeature::Live)
        {
            features.push(crate::innertube::SearchFeature::Live);
        }
        let result_type = if self.result_type != crate::innertube::SearchResultType::Any {
            self.result_type
        } else {
            match self.kind {
                SearchTypeFilter::All | SearchTypeFilter::Shorts | SearchTypeFilter::Live => {
                    crate::innertube::SearchResultType::Any
                }
                SearchTypeFilter::Videos => crate::innertube::SearchResultType::Video,
                SearchTypeFilter::Channels => crate::innertube::SearchResultType::Channel,
                SearchTypeFilter::Playlists => crate::innertube::SearchResultType::Playlist,
                SearchTypeFilter::Movies => crate::innertube::SearchResultType::Movie,
            }
        };
        crate::innertube::SearchFilters {
            upload_date: self.upload_date,
            result_type,
            duration: self.duration,
            features,
            sort_by: match self.sort {
                SearchSort::Relevance => crate::innertube::SearchSortBy::Relevance,
                SearchSort::Date => crate::innertube::SearchSortBy::UploadDate,
                SearchSort::Views => crate::innertube::SearchSortBy::ViewCount,
                SearchSort::Rating => crate::innertube::SearchSortBy::Rating,
            },
        }
    }

    #[allow(dead_code)]
    pub fn from_innertube(filters: &crate::innertube::SearchFilters) -> Self {
        Self {
            sort: match filters.sort_by {
                crate::innertube::SearchSortBy::Relevance => SearchSort::Relevance,
                crate::innertube::SearchSortBy::UploadDate => SearchSort::Date,
                crate::innertube::SearchSortBy::ViewCount => SearchSort::Views,
                crate::innertube::SearchSortBy::Rating => SearchSort::Rating,
            },
            upload_date: filters.upload_date,
            result_type: filters.result_type,
            duration: filters.duration,
            features: filters.features.clone(),
            kind: match filters.result_type {
                crate::innertube::SearchResultType::Any => SearchTypeFilter::All,
                crate::innertube::SearchResultType::Video => SearchTypeFilter::Videos,
                crate::innertube::SearchResultType::Channel => SearchTypeFilter::Channels,
                crate::innertube::SearchResultType::Playlist => SearchTypeFilter::Playlists,
                crate::innertube::SearchResultType::Movie => SearchTypeFilter::Movies,
            },
        }
    }

    #[allow(dead_code)]
    pub fn has_feature(&self, feature: crate::innertube::SearchFeature) -> bool {
        self.features.contains(&feature)
    }

    #[allow(dead_code)]
    pub fn toggle_feature(&mut self, feature: crate::innertube::SearchFeature) {
        if let Some(pos) = self.features.iter().position(|f| *f == feature) {
            self.features.remove(pos);
        } else {
            self.features.push(feature);
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchChip {
    #[default]
    All,
    Shorts,
    Unwatched,
    Videos,
    RecentlyUploaded,
    Live,
}

impl SearchChip {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "Tudo",
            Self::Shorts => "Shorts",
            Self::Unwatched => "N\u{00e3}o assistidos",
            Self::Videos => "V\u{00ed}deos",
            Self::RecentlyUploaded => "Enviados recentemente",
            Self::Live => "Ao vivo",
        }
    }
}

pub fn effective_search_filters<F: std::borrow::Borrow<SearchFilters>>(
    base: F,
    chip: SearchChip,
) -> SearchFilters {
    let mut base = base.borrow().clone();
    match chip {
        SearchChip::All | SearchChip::Unwatched => {}
        SearchChip::Shorts => {
            base.kind = SearchTypeFilter::Shorts;
        }
        SearchChip::Videos => {
            base.kind = SearchTypeFilter::Videos;
            if base.result_type == crate::innertube::SearchResultType::Any {
                base.result_type = crate::innertube::SearchResultType::Video;
            }
        }
        SearchChip::RecentlyUploaded => {
            if base.sort == SearchSort::Relevance {
                base.sort = SearchSort::Date;
            }
        }
        SearchChip::Live => {
            if !base
                .features
                .contains(&crate::innertube::SearchFeature::Live)
            {
                base.features.push(crate::innertube::SearchFeature::Live);
            }
        }
    }
    base
}

#[allow(dead_code)]
fn push_proto_varint(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[allow(dead_code)]
fn push_proto_field_varint(buf: &mut Vec<u8>, field_number: u32, value: u64) {
    push_proto_varint(buf, u64::from(field_number) << 3);
    push_proto_varint(buf, value);
}

/// Encodes `SearchFilters` into YouTube's `sp` base64 protobuf query parameter.
#[allow(dead_code)]
pub fn encode_search_sp<F: std::borrow::Borrow<SearchFilters>>(filters: F) -> Option<String> {
    use base64::Engine as _;

    let filters = filters.borrow();
    if filters.kind == SearchTypeFilter::Shorts
        && filters.result_type == crate::innertube::SearchResultType::Any
        && filters.upload_date == crate::innertube::SearchUploadDate::Any
        && filters.duration == crate::innertube::SearchDuration::Any
        && filters.features.is_empty()
        && filters.sort == SearchSort::Relevance
    {
        let mut msg = Vec::new();
        let mut sub = Vec::new();
        push_proto_field_varint(&mut sub, 2, 9);
        push_proto_varint(&mut msg, (2 << 3) | 2);
        push_proto_varint(&mut msg, sub.len() as u64);
        msg.extend_from_slice(&sub);
        return Some(base64::engine::general_purpose::STANDARD.encode(msg));
    }

    crate::innertube::encode_search_sp(&filters.to_innertube())
}

pub fn parse_duration_label_secs(label: &str) -> Option<u64> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split(':');
    let a = parts.next()?.parse::<u64>().ok()?;
    let b = parts.next()?.parse::<u64>().ok()?;
    match parts.next() {
        None => Some(a * 60 + b),
        Some(c_str) => {
            if parts.next().is_some() {
                return None;
            }
            let c = c_str.parse::<u64>().ok()?;
            Some(a * 3600 + b * 60 + c)
        }
    }
}

pub fn apply_search_filters<F: std::borrow::Borrow<SearchFilters>>(
    mut results: Vec<SearchResult>,
    filters: F,
) -> Vec<SearchResult> {
    let filters = filters.borrow();
    let wants_live = filters
        .features
        .contains(&crate::innertube::SearchFeature::Live)
        || filters.kind == SearchTypeFilter::Live;

    results.retain(|item| {
        let secs = parse_duration_label_secs(&item.video.duration);
        let is_short = item.metadata.kind == SearchResultKind::Short
            || item.video.url.contains("/shorts/")
            || secs.is_some_and(|s| s > 0 && s <= 60);
        let is_live = item.metadata.is_live
            || (item.metadata.kind == SearchResultKind::Video
                && item.video.duration.trim().is_empty());

        let kind_ok = if filters.result_type != crate::innertube::SearchResultType::Any {
            match filters.result_type {
                crate::innertube::SearchResultType::Any => true,
                crate::innertube::SearchResultType::Video => {
                    item.metadata.kind == SearchResultKind::Video && !is_short && !is_live
                }
                crate::innertube::SearchResultType::Channel => {
                    item.metadata.kind == SearchResultKind::Channel
                }
                crate::innertube::SearchResultType::Playlist => {
                    item.metadata.kind == SearchResultKind::Playlist
                }
                crate::innertube::SearchResultType::Movie => {
                    item.metadata.kind == SearchResultKind::Movie
                }
            }
        } else {
            match filters.kind {
                SearchTypeFilter::All => true,
                SearchTypeFilter::Videos => {
                    item.metadata.kind == SearchResultKind::Video && !is_short && !is_live
                }
                SearchTypeFilter::Shorts => is_short,
                SearchTypeFilter::Channels => item.metadata.kind == SearchResultKind::Channel,
                SearchTypeFilter::Playlists => item.metadata.kind == SearchResultKind::Playlist,
                SearchTypeFilter::Movies => item.metadata.kind == SearchResultKind::Movie,
                SearchTypeFilter::Live => is_live,
            }
        };
        if !kind_ok {
            return false;
        }
        if wants_live && !is_live {
            return false;
        }
        match filters.duration {
            crate::innertube::SearchDuration::Any => true,
            crate::innertube::SearchDuration::Under4Min => secs.is_some_and(|s| s < 240),
            crate::innertube::SearchDuration::Between4And20Min => {
                secs.is_some_and(|s| (240..=1200).contains(&s))
            }
            crate::innertube::SearchDuration::Over20Min => secs.is_some_and(|s| s > 1200),
        }
    });
    if filters.sort == SearchSort::Views {
        results.sort_by(|a, b| {
            b.metadata
                .view_count
                .unwrap_or(0)
                .cmp(&a.metadata.view_count.unwrap_or(0))
        });
    }
    results
}
