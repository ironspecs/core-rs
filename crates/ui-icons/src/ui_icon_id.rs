//! `UiIconId` owns the stable icon ids shared by the atlas generator and both app shells.
//!
//! The icon ordering is the stable contract used by atlas generation and atlas
//! lookup. The total curated count constant lives in `lib.rs` as `UI_ICON_COUNT`.
//!
//! Strict invariants:
//! - this file owns only the `UiIconId` enum and its `impl` blocks.

/// Stable icon ids shared by the atlas generator and both app shells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub enum UiIconId {
    Scene = 0,
    Media = 1,
    Control = 2,
    Transport = 3,
    Persistence = 4,
    Diagnostics = 5,
    Avatar = 6,
    Tracking = 7,
    Shader = 8,
    Shared = 9,
    PubSub = 10,
    Hamburger = 11,
    Audio = 12,
    CaptureUnavailable = 13,
    Generic = 14,
    Plus = 15,
    Minus = 16,
    Camera = 17,
    Close = 18,
    Ellipsis = 19,
}

impl UiIconId {
    /// Stable icon ordering used by atlas generation and atlas lookup.
    pub const ALL: [Self; 20] = [
        Self::Scene,
        Self::Media,
        Self::Control,
        Self::Transport,
        Self::Persistence,
        Self::Diagnostics,
        Self::Avatar,
        Self::Tracking,
        Self::Shader,
        Self::Shared,
        Self::PubSub,
        Self::Hamburger,
        Self::Audio,
        Self::CaptureUnavailable,
        Self::Generic,
        Self::Plus,
        Self::Minus,
        Self::Camera,
        Self::Close,
        Self::Ellipsis,
    ];

    /// Returns the stable numeric id for this icon.
    #[must_use]
    pub const fn numeric_id(self) -> u16 {
        self as u16
    }

    /// Returns the stable dense array index for this icon.
    #[must_use]
    pub const fn dense_index(self) -> usize {
        self as usize
    }

    /// Returns the source SVG file stem for this icon.
    #[must_use]
    pub const fn file_stem(self) -> &'static str {
        match self {
            Self::Scene => "scene",
            Self::Media => "media",
            Self::Control => "control",
            Self::Transport => "transport",
            Self::Persistence => "persistence",
            Self::Diagnostics => "diagnostics",
            Self::Avatar => "avatar",
            Self::Tracking => "tracking",
            Self::Shader => "shader",
            Self::Shared => "shared",
            Self::PubSub => "pubsub",
            Self::Hamburger => "hamburger",
            Self::Audio => "audio",
            Self::CaptureUnavailable => "capture-unavailable",
            Self::Generic => "generic",
            Self::Plus => "plus",
            Self::Minus => "minus",
            Self::Camera => "camera",
            Self::Close => "close",
            Self::Ellipsis => "ellipsis",
        }
    }

    /// Resolves one stable icon id from its source SVG file stem.
    #[must_use]
    pub fn from_file_stem(value: &str) -> Option<Self> {
        match value {
            "scene" => Some(Self::Scene),
            "media" => Some(Self::Media),
            "control" => Some(Self::Control),
            "transport" => Some(Self::Transport),
            "persistence" => Some(Self::Persistence),
            "diagnostics" => Some(Self::Diagnostics),
            "avatar" => Some(Self::Avatar),
            "tracking" => Some(Self::Tracking),
            "shader" => Some(Self::Shader),
            "shared" => Some(Self::Shared),
            "pubsub" => Some(Self::PubSub),
            "hamburger" => Some(Self::Hamburger),
            "audio" => Some(Self::Audio),
            "capture-unavailable" => Some(Self::CaptureUnavailable),
            "generic" => Some(Self::Generic),
            "plus" => Some(Self::Plus),
            "minus" => Some(Self::Minus),
            "camera" => Some(Self::Camera),
            "close" => Some(Self::Close),
            "ellipsis" => Some(Self::Ellipsis),
            _ => None,
        }
    }

    /// Resolves one stable icon id from its numeric manifest id.
    #[must_use]
    pub const fn from_numeric_id(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Scene),
            1 => Some(Self::Media),
            2 => Some(Self::Control),
            3 => Some(Self::Transport),
            4 => Some(Self::Persistence),
            5 => Some(Self::Diagnostics),
            6 => Some(Self::Avatar),
            7 => Some(Self::Tracking),
            8 => Some(Self::Shader),
            9 => Some(Self::Shared),
            10 => Some(Self::PubSub),
            11 => Some(Self::Hamburger),
            12 => Some(Self::Audio),
            13 => Some(Self::CaptureUnavailable),
            14 => Some(Self::Generic),
            15 => Some(Self::Plus),
            16 => Some(Self::Minus),
            17 => Some(Self::Camera),
            18 => Some(Self::Close),
            19 => Some(Self::Ellipsis),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_order_matches_unique_file_stems() {
        let mut stems = UiIconId::ALL
            .iter()
            .map(|icon| icon.file_stem())
            .collect::<Vec<_>>();
        stems.sort_unstable();
        stems.dedup();

        assert_eq!(stems.len(), UiIconId::ALL.len());
    }

    #[test]
    fn file_stem_round_trips_to_icon_ids() {
        for icon in UiIconId::ALL {
            assert_eq!(UiIconId::from_file_stem(icon.file_stem()), Some(icon));
        }
        assert_eq!(UiIconId::from_file_stem("not-a-real-icon"), None);
    }

    #[test]
    fn numeric_ids_round_trip_to_icon_ids() {
        for icon in UiIconId::ALL {
            assert_eq!(UiIconId::from_numeric_id(icon.numeric_id()), Some(icon));
        }
        assert_eq!(UiIconId::from_numeric_id(99), None);
    }
}
