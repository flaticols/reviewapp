//! Small presentational elements shared by Switchfold's screens: counters, initials
//! avatars, tinted status tags, state icons, section headers and empty states.

mod actor_name;
mod assets;
mod avatar;
mod counter;
mod empty_state;
mod filter_tabs;
mod section_header;
mod settings_parts;
mod status;
mod status_tag;

pub use actor_name::ActorName;
pub use assets::AppAssets;
pub use avatar::{AvatarSize, InitialsAvatar};
pub use counter::Counter;
pub use empty_state::EmptyState;
pub use filter_tabs::FilterTabs;
pub use section_header::SectionHeader;
pub use settings_parts::{SettingsPage, SettingsRow, SettingsSection};
pub use status::{CheckIcon, PullRequestStateIcon, check_color};
pub use status_tag::{StatusTag, Tone};

pub use gpui_kit::assets::IconName;
