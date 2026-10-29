use std::{
    collections::{HashMap, VecDeque},
    time::Instant,
};

use lspotify_core::{CliArtworkPreference, LocalPlaybackBackendPreference};

pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LayoutMode {
    Resize,
    Compact,
    #[default]
    Standard,
    Wide,
    SidePlayer,
    StackedQueue,
}

impl LayoutMode {
    pub fn for_size(width: u16, height: u16) -> Self {
        Self::for_size_with_settings(width, height, 28, 100, 140, 38)
    }

    pub fn for_size_with_settings(
        width: u16,
        height: u16,
        side_player_max_height: u16,
        side_player_min_width: u16,
        wide_breakpoint_width: u16,
        stacked_queue_min_height: u16,
    ) -> Self {
        if width < MIN_WIDTH || height < MIN_HEIGHT {
            Self::Resize
        } else if side_player_max_height > 0
            && height <= side_player_max_height
            && width >= side_player_min_width
        {
            Self::SidePlayer
        } else if width >= wide_breakpoint_width {
            Self::Wide
        } else if stacked_queue_min_height <= 500 && height >= stacked_queue_min_height {
            Self::StackedQueue
        } else if width < 100 {
            Self::Compact
        } else {
            Self::Standard
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FocusRegion {
    Sidebar,
    #[default]
    Content,
    Queue,
    Player,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LibraryTab {
    #[default]
    LikedSongs,
    Albums,
    Playlists,
}

impl LibraryTab {
    pub const ALL: [Self; 3] = [Self::LikedSongs, Self::Albums, Self::Playlists];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LikedSongs => "Liked Songs",
            Self::Albums => "Albums",
            Self::Playlists => "Playlists",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::LikedSongs => Self::Albums,
            Self::Albums => Self::Playlists,
            Self::Playlists => Self::LikedSongs,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::LikedSongs => Self::Playlists,
            Self::Albums => Self::LikedSongs,
            Self::Playlists => Self::Albums,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SettingsTab {
    #[default]
    General,
    Playback,
    Shortcuts,
}

impl SettingsTab {
    pub const ALL: [Self; 3] = [Self::General, Self::Playback, Self::Shortcuts];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Playback => "Playback",
            Self::Shortcuts => "Shortcuts",
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::General => Self::Playback,
            Self::Playback => Self::Shortcuts,
            Self::Shortcuts => Self::General,
        }
    }

    #[must_use]
    pub const fn previous(self) -> Self {
        match self {
            Self::General => Self::Shortcuts,
            Self::Playback => Self::General,
            Self::Shortcuts => Self::Playback,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchFilter {
    #[default]
    All,
    Tracks,
    Albums,
    Artists,
    Playlists,
}

impl SearchFilter {
    pub const ALL: [Self; 5] =
        [Self::All, Self::Tracks, Self::Albums, Self::Artists, Self::Playlists];
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Tracks => "Tracks",
            Self::Albums => "Albums",
            Self::Artists => "Artists",
            Self::Playlists => "Playlists",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Route {
    #[default]
    Home,
    Search,
    Library,
    Album {
        uri: String,
        title: String,
    },
    Playlist {
        uri: String,
        title: String,
    },
    Artist {
        uri: String,
        title: String,
    },
    Queue,
    Devices,
    Settings,
}

impl Route {
    pub fn label(&self) -> &str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Library => "Library",
            Self::Album { title, .. }
            | Self::Playlist { title, .. }
            | Self::Artist { title, .. } => title,
            Self::Queue => "Queue",
            Self::Devices => "Devices",
            Self::Settings => "Settings",
        }
    }
    pub fn key(&self) -> String {
        match self {
            Self::Home => "home".into(),
            Self::Search => "search".into(),
            Self::Library => "library".into(),
            Self::Album { uri, .. } => format!("album:{uri}"),
            Self::Playlist { uri, .. } => format!("playlist:{uri}"),
            Self::Artist { uri, .. } => format!("artist:{uri}"),
            Self::Queue => "queue".into(),
            Self::Devices => "devices".into(),
            Self::Settings => "settings".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityKind {
    Track,
    Album,
    Artist,
    Playlist,
    Device,
    Action,
    Message,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPosition {
    pub uri: String,
    pub position: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseItem {
    pub id: String,
    pub kind: EntityKind,
    pub title: String,
    pub subtitle: String,
    pub metadata: String,
    pub uri: Option<String>,
    pub external_url: Option<String>,
    pub artwork_url: Option<String>,
    pub artists: Vec<(String, Option<String>)>,
    pub album: Option<(String, String)>,
    pub duration_ms: Option<u64>,
    pub available: bool,
    pub context: Option<ContextPosition>,
    pub restricted: bool,
    pub saved: Option<bool>,
}

impl BrowseItem {
    pub fn message(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: EntityKind::Message,
            title: text.into(),
            subtitle: String::new(),
            metadata: String::new(),
            uri: None,
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: None,
            available: false,
            context: None,
            restricted: false,
            saved: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Section {
    pub title: String,
    pub items: Vec<BrowseItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LoadState {
    #[default]
    Loading,
    Ready,
    Empty(String),
    Partial(String),
    Restricted(String),
    Failed(String),
    Stale(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ListCursor {
    pub selected: usize,
    pub offset: usize,
}

impl ListCursor {
    pub fn move_by(&mut self, delta: isize, len: usize, viewport: usize) {
        if len == 0 {
            self.selected = 0;
            self.offset = 0;
            return;
        }
        self.selected = self.selected.saturating_add_signed(delta).min(len - 1);
        self.reveal(viewport, len);
    }
    pub fn move_to(&mut self, index: usize, len: usize, viewport: usize) {
        self.selected = index.min(len.saturating_sub(1));
        self.reveal(viewport, len);
    }
    pub fn reveal(&mut self, viewport: usize, len: usize) {
        let viewport = viewport.max(1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset.saturating_add(viewport) {
            self.offset = self.selected + 1 - viewport;
        }
        self.offset = self.offset.min(len.saturating_sub(viewport));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageState {
    pub route: Route,
    pub title: String,
    pub subtitle: String,
    pub artwork_url: Option<String>,
    pub uri: Option<String>,
    pub external_url: Option<String>,
    pub sections: Vec<Section>,
    pub cursor: ListCursor,
    pub state: LoadState,
    pub filter: String,
    pub library_tab: LibraryTab,
    pub settings_tab: SettingsTab,
    pub search_filter: SearchFilter,
    pub next_offset: Option<u32>,
    pub loading_more: bool,
    pub generation: u64,
}

impl PageState {
    pub fn loading(route: Route, generation: u64) -> Self {
        let title = route.label().to_owned();
        let uri = match &route {
            Route::Album { uri, .. } | Route::Playlist { uri, .. } | Route::Artist { uri, .. } => {
                Some(uri.clone())
            }
            _ => None,
        };
        let external_url = uri.as_deref().and_then(|u| {
            u.parse::<lspotify_core::ids::SpotifyUri>().ok().map(|parsed| parsed.web_url())
        });
        Self {
            route,
            title,
            subtitle: String::new(),
            artwork_url: None,
            uri,
            external_url,
            sections: Vec::new(),
            cursor: ListCursor::default(),
            state: LoadState::Loading,
            filter: String::new(),
            library_tab: LibraryTab::LikedSongs,
            settings_tab: SettingsTab::General,
            search_filter: SearchFilter::All,
            next_offset: None,
            loading_more: false,
            generation,
        }
    }
    pub fn cache_key(&self) -> String {
        match self.route {
            Route::Library => match self.library_tab {
                LibraryTab::LikedSongs => "library:liked".into(),
                LibraryTab::Albums => "library:albums".into(),
                LibraryTab::Playlists => "library:playlists".into(),
            },
            _ => self.route.key(),
        }
    }
    pub fn flattened(&self) -> Vec<&BrowseItem> {
        let needle = self.filter.trim().to_lowercase();
        self.sections
            .iter()
            .flat_map(|section| &section.items)
            .filter(|item| {
                (needle.is_empty()
                    || item.title.to_lowercase().contains(&needle)
                    || item.subtitle.to_lowercase().contains(&needle))
                    && match self.route {
                        Route::Library => match self.library_tab {
                            LibraryTab::LikedSongs => item.kind == EntityKind::Track,
                            LibraryTab::Albums => item.kind == EntityKind::Album,
                            LibraryTab::Playlists => item.kind == EntityKind::Playlist,
                        },
                        Route::Search => match self.search_filter {
                            SearchFilter::All => true,
                            SearchFilter::Tracks => item.kind == EntityKind::Track,
                            SearchFilter::Albums => item.kind == EntityKind::Album,
                            SearchFilter::Artists => item.kind == EntityKind::Artist,
                            SearchFilter::Playlists => item.kind == EntityKind::Playlist,
                        },
                        _ => true,
                    }
            })
            .collect()
    }
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        self.flattened().get(self.cursor.selected).copied()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackState {
    pub track_uri: Option<String>,
    pub context_uri: Option<String>,
    pub title: String,
    pub artist: String,
    pub artists: Vec<(String, Option<String>)>,
    pub album: String,
    pub album_uri: Option<String>,
    pub artwork_url: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub playing: bool,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume: u8,
    pub shuffle: bool,
    pub repeat: u8,
    pub available_actions: Vec<String>,
    pub observed_at: Option<Instant>,
    /// Metadata restored from the CLI sidecar, rather than a live Spotify snapshot.
    pub cached_track: bool,
}

impl PlaybackState {
    pub fn progress_at(&self, now: Instant) -> u64 {
        let elapsed = if self.playing {
            self.observed_at.map_or(0, |seen| {
                u64::try_from(now.saturating_duration_since(seen).as_millis()).unwrap_or(u64::MAX)
            })
        } else {
            0
        };
        if self.duration_ms > 0 {
            self.progress_ms.saturating_add(elapsed).min(self.duration_ms)
        } else {
            self.progress_ms.saturating_add(elapsed)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextAction {
    Play,
    PlayNext,
    AddToQueue,
    Like,
    Unlike,
    AddToPlaylist,
    RemoveFromPlaylist,
    SaveAlbum,
    RemoveSavedAlbum,
    FollowArtist,
    UnfollowArtist,
    PlayContext,
    ShuffleContext,
    GoToAlbum,
    GoToArtist,
    RenamePlaylist,
    DeletePlaylist,
    OpenInSpotify,
    EditShortcut,
    RemoveShortcutBinding,
    ResetShortcutToDefault,
    ResetAllShortcutsToDefault,
}

impl ContextAction {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Play | Self::PlayContext => "Play",
            Self::PlayNext => "Play next",
            Self::AddToQueue => "Add to queue",
            Self::Like => "Like",
            Self::Unlike => "Unlike",
            Self::AddToPlaylist => "Add to playlist…",
            Self::RemoveFromPlaylist => "Remove from this playlist",
            Self::SaveAlbum => "Save album",
            Self::RemoveSavedAlbum => "Remove from library",
            Self::FollowArtist => "Follow artist",
            Self::UnfollowArtist => "Unfollow artist",
            Self::ShuffleContext => "Shuffle",
            Self::GoToAlbum => "Go to album",
            Self::GoToArtist => "Go to artist",
            Self::RenamePlaylist => "Rename playlist",
            Self::DeletePlaylist => "Delete playlist",
            Self::OpenInSpotify => "Open in Spotify",
            Self::EditShortcut => "Assign key…",
            Self::RemoveShortcutBinding => "Remove binding",
            Self::ResetShortcutToDefault => "Restore to default",
            Self::ResetAllShortcutsToDefault => "Restore all shortcuts to defaults",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Overlay {
    Menu,
    Help {
        query: String,
        editing: bool,
    },
    Actions {
        selected: usize,
        actions: Vec<ContextAction>,
    },
    Inspector,
    DevicePicker,
    PlaylistPicker {
        selected: usize,
        playlists: Vec<BrowseItem>,
        pending_uri: String,
    },
    RenamePlaylist {
        playlist_id: String,
        name: String,
    },
    ShortcutCapture {
        action: crate::shortcuts::ShortcutAction,
        conflict: Option<(crate::shortcuts::KeyBinding, crate::shortcuts::ShortcutAction)>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NoticeKind {
    Info,
    Pending,
    Success,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub text: String,
}

fn default_local_playback_backend() -> LocalPlaybackBackendPreference {
    if cfg!(target_os = "windows") {
        LocalPlaybackBackendPreference::SpotifyWeb
    } else {
        LocalPlaybackBackendPreference::Librespot
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
pub struct AppState {
    pub page: PageState,
    pub library_tab: LibraryTab,
    pub settings_tab: SettingsTab,
    pub history: Vec<PageState>,
    pub focus: FocusRegion,
    pub sidebar: ListCursor,
    /// Primary section retained while browsing a detail page for top-bar highlighting.
    pub primary_section: Route,
    pub queue: ListCursor,
    pub queue_now: Option<BrowseItem>,
    pub queue_upcoming: Vec<BrowseItem>,
    pub queue_state: LoadState,
    pub playback: PlaybackState,
    pub overlay: Option<Overlay>,
    pub text_entry: bool,
    pub search_query: String,
    pub layout: LayoutMode,
    pub terminal_size: (u16, u16),
    pub side_player_max_height: u16,
    pub side_player_min_width: u16,
    pub stacked_queue_min_height: u16,
    pub wide_breakpoint_width: u16,
    pub content_height: usize,
    /// Number of visible upcoming-queue rows in the rendered queue panel.
    pub queue_height: usize,
    pub generation: u64,
    pub notice: Option<Notice>,
    pub backend_error: Option<Notice>,
    pub queue_refresh_at: Option<Instant>,
    pub queue_fetched_track_uri: Option<String>,
    pub optimistic_transport: crate::app::OptimisticTransport,
    pub quit: bool,
    pub artwork: CliArtworkPreference,
    pub mouse: bool,
    pub wide_queue: bool,
    /// Device registered by the selected local playback backend, when available.
    pub local_device_id: Option<String>,
    pub local_playback_backend: LocalPlaybackBackendPreference,
    pub local_start_requested: bool,
    pub selected_device_id: Option<String>,
    /// A device chosen from the Devices page must not be replaced automatically.
    pub device_selected_by_user: bool,
    pub response_log: VecDeque<u64>,
    pub playback_history: Vec<PlaybackState>,
    pub recent_searches: Vec<String>,
    pub page_cache: HashMap<String, (PageState, Instant)>,
    pub startup_focus_pending: bool,
    pub liked_songs_warmed: bool,
    pub artwork_under_overlays: bool,
    pub shortcuts: crate::shortcuts::ShortcutRegistry,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            page: PageState::loading(Route::Home, 0),
            library_tab: LibraryTab::LikedSongs,
            settings_tab: SettingsTab::General,
            history: Vec::new(),
            focus: FocusRegion::Content,
            sidebar: ListCursor::default(),
            primary_section: Route::Home,
            queue: ListCursor::default(),
            queue_now: None,
            queue_upcoming: Vec::new(),
            queue_state: LoadState::Loading,
            playback: PlaybackState { volume: 50, ..PlaybackState::default() },
            overlay: None,
            text_entry: false,
            search_query: String::new(),
            layout: LayoutMode::Standard,
            terminal_size: (120, 30),
            side_player_max_height: 28,
            side_player_min_width: 100,
            stacked_queue_min_height: 38,
            wide_breakpoint_width: 140,
            content_height: 10,
            queue_height: 1,
            generation: 0,
            notice: None,
            backend_error: None,
            queue_refresh_at: None,
            queue_fetched_track_uri: None,
            optimistic_transport: crate::app::OptimisticTransport::default(),
            quit: false,
            artwork: CliArtworkPreference::Auto,
            mouse: true,
            wide_queue: true,
            local_device_id: None,
            local_playback_backend: default_local_playback_backend(),
            local_start_requested: false,
            selected_device_id: None,
            device_selected_by_user: false,
            response_log: VecDeque::new(),
            playback_history: Vec::new(),
            recent_searches: Vec::new(),
            page_cache: HashMap::new(),
            startup_focus_pending: true,
            liked_songs_warmed: false,
            artwork_under_overlays: false,
            shortcuts: crate::shortcuts::ShortcutRegistry::default(),
        }
    }
}

impl AppState {
    pub const NAVIGATION: [Route; 6] =
        [Route::Home, Route::Search, Route::Library, Route::Queue, Route::Devices, Route::Settings];
    pub fn update_layout(&mut self, width: u16, height: u16) {
        if width > 0 && height > 0 {
            self.terminal_size = (width, height);
        }
        let (w, h) = if self.terminal_size.0 > 0 && self.terminal_size.1 > 0 {
            self.terminal_size
        } else {
            (MIN_WIDTH, MIN_HEIGHT)
        };
        self.layout = LayoutMode::for_size_with_settings(
            w,
            h,
            self.side_player_max_height,
            self.side_player_min_width,
            self.wide_breakpoint_width,
            self.stacked_queue_min_height,
        );
        if (self.layout == LayoutMode::Compact
            || self.layout == LayoutMode::SidePlayer
            || self.layout == LayoutMode::StackedQueue)
            && self.focus == FocusRegion::Sidebar
        {
            self.focus = FocusRegion::Content;
        }
        if !self.queue_visible() && self.focus == FocusRegion::Queue {
            self.focus = FocusRegion::Content;
        }
    }
    /// Returns true if this layout mode renders a persistent on-screen queue widget.
    pub fn layout_has_inline_queue(&self) -> bool {
        (self.layout == LayoutMode::Wide || self.layout == LayoutMode::StackedQueue)
            && self.wide_queue
    }
    /// Returns true if the inline queue pane is actively being rendered right now.
    pub fn queue_visible(&self) -> bool {
        self.layout_has_inline_queue() && self.page.route != Route::Queue
    }
    /// Returns the most specific target device identifier available for playback commands.
    pub fn target_device_id(&self) -> Option<String> {
        self.selected_device_id
            .clone()
            .or_else(|| self.local_device_id.clone())
            .or_else(|| self.playback.device_id.clone())
    }
    /// Returns a human-friendly device label for the status deck.
    pub fn display_device_name(&self) -> &str {
        if let Some(ref name) = self.playback.device_name
            && !name.is_empty()
        {
            return name.as_str();
        }
        if self.local_device_id.is_some()
            && (self.selected_device_id.is_none()
                || self.selected_device_id == self.local_device_id)
        {
            return "lspotify";
        }
        if self.selected_device_id.is_some() {
            return "Spotify Connect";
        }
        "choose with d"
    }
    /// Clears any transient error notice.
    pub fn clear_error_notice(&mut self) {
        if self.notice.as_ref().is_some_and(|n| n.kind == NoticeKind::Error) {
            self.notice = None;
        }
    }
    pub fn navigate(&mut self, route: Route) -> u64 {
        if self.page.route != route {
            if matches!(
                route,
                Route::Home | Route::Search | Route::Library | Route::Settings | Route::Queue
            ) {
                self.primary_section = route.clone();
            }
            self.history.push(self.page.clone());
        }
        self.generation = self.generation.saturating_add(1);
        let cache_key = match route {
            Route::Library => match self.library_tab {
                LibraryTab::LikedSongs => "library:liked".into(),
                LibraryTab::Albums => "library:albums".into(),
                LibraryTab::Playlists => "library:playlists".into(),
            },
            _ => route.key(),
        };
        if let Some((cached, _)) = self.page_cache.get(&cache_key) {
            let mut page = cached.clone();
            page.generation = self.generation;
            page.library_tab = self.library_tab;
            self.page = page;
        } else {
            let mut page = PageState::loading(route, self.generation);
            page.library_tab = self.library_tab;
            self.page = page;
        }
        self.overlay = None;
        self.text_entry = false;
        self.startup_focus_pending = false;
        self.clear_error_notice();
        self.generation
    }
    pub fn populate_recent_searches(&mut self) {
        self.page.title = "Search".into();
        self.page.subtitle = if self.recent_searches.is_empty() {
            if self.text_entry {
                "Type query and press Enter to search Spotify".into()
            } else {
                "Press / to type a search query".into()
            }
        } else if self.text_entry {
            "Type query, press Enter to search, or ↓ for recent searches".into()
        } else {
            "Press / to type, or select a recent search".into()
        };
        if self.recent_searches.is_empty() {
            self.page.sections = Vec::new();
            self.page.state =
                LoadState::Empty("Type a query and press Enter to search Spotify.".into());
        } else {
            let items = self
                .recent_searches
                .iter()
                .map(|query| BrowseItem {
                    id: format!("recent-search:{query}"),
                    kind: EntityKind::Action,
                    title: query.clone(),
                    subtitle: "Recent search · press Enter to search again".into(),
                    metadata: "Enter searches Spotify with this query.".into(),
                    uri: None,
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: true,
                    context: None,
                    restricted: false,
                    saved: None,
                })
                .collect();
            self.page.sections = vec![Section { title: "Recent searches".into(), items }];
            self.page.state = LoadState::Ready;
        }
    }
    pub fn back(&mut self) {
        if let Some(page) = self.history.pop() {
            self.generation = self.generation.saturating_add(1);
            if matches!(
                page.route,
                Route::Home | Route::Search | Route::Library | Route::Settings | Route::Queue
            ) {
                self.primary_section = page.route.clone();
            }
            self.page = page;
            self.page.generation = self.generation;
            self.overlay = None;
            self.text_entry = false;
        }
    }
    pub fn visible_focuses(&self) -> Vec<FocusRegion> {
        let mut regions = Vec::new();
        if self.layout != LayoutMode::Compact
            && self.layout != LayoutMode::SidePlayer
            && self.layout != LayoutMode::StackedQueue
        {
            regions.push(FocusRegion::Sidebar);
        }
        regions.push(FocusRegion::Content);
        if self.queue_visible() {
            regions.push(FocusRegion::Queue);
        }
        regions.push(FocusRegion::Player);
        regions
    }
    pub fn cycle_focus(&mut self, reverse: bool) {
        let regions = self.visible_focuses();
        let position = regions.iter().position(|region| *region == self.focus).unwrap_or(0);
        let next = if reverse {
            position.checked_sub(1).unwrap_or(regions.len() - 1)
        } else {
            (position + 1) % regions.len()
        };
        self.focus = regions[next];
    }
    pub fn selected_item(&self) -> Option<&BrowseItem> {
        match self.focus {
            FocusRegion::Queue => self.queue_upcoming.get(self.queue.selected),
            _ => self.page.selected_item(),
        }
    }
    pub fn accept_page(&mut self, mut page: PageState) -> bool {
        if page.generation != self.generation || page.route.key() != self.page.route.key() {
            return false;
        }
        if page.state == LoadState::Ready || matches!(page.state, LoadState::Partial(_)) {
            self.page_cache.insert(page.cache_key(), (page.clone(), Instant::now()));
        }
        let is_valid_home = page.route == Route::Home
            && matches!(page.state, LoadState::Ready | LoadState::Partial(_) | LoadState::Empty(_));
        if self.startup_focus_pending && is_valid_home {
            self.focus = FocusRegion::Content;
            let len = page.flattened().len();
            page.cursor.selected = 0;
            page.cursor.offset = 0;
            if len > 0 {
                page.cursor.reveal(self.content_height, len);
            }
            self.startup_focus_pending = false;
        } else if self.page.route.key() == page.route.key() {
            let prev_cursor = self.page.cursor.clone();
            if prev_cursor.selected > 0 || prev_cursor.offset > 0 {
                let len = page.flattened().len();
                page.cursor = prev_cursor;
                if len == 0 {
                    page.cursor.selected = 0;
                    page.cursor.offset = 0;
                } else if page.cursor.selected >= len {
                    page.cursor.selected = len - 1;
                    page.cursor.reveal(self.content_height, len);
                } else {
                    page.cursor.reveal(self.content_height, len);
                }
            }
        }
        self.page = page;
        self.response_log.push_back(self.generation);
        while self.response_log.len() > 16 {
            self.response_log.pop_front();
        }
        true
    }
    pub fn push_playback_history(&mut self) {
        if let Some(uri) = &self.playback.track_uri
            && !self.playback.title.is_empty()
        {
            if self.playback_history.last().and_then(|p| p.track_uri.as_ref()) == Some(uri) {
                return;
            }
            if self.playback_history.len() >= 50 {
                self.playback_history.remove(0);
            }
            self.playback_history.push(self.playback.clone());
        }
    }
    #[must_use]
    pub fn playback_as_browse_item(&self) -> Option<BrowseItem> {
        let uri = self.playback.track_uri.clone()?;
        if self.playback.title.is_empty() {
            return None;
        }
        Some(BrowseItem {
            id: uri.clone(),
            kind: EntityKind::Track,
            title: self.playback.title.clone(),
            subtitle: self.playback.artist.clone(),
            metadata: String::new(),
            uri: Some(uri),
            external_url: None,
            artwork_url: self.playback.artwork_url.clone(),
            artists: self.playback.artists.clone(),
            album: self
                .playback
                .album_uri
                .clone()
                .map(|album_uri| (self.playback.album.clone(), album_uri)),
            duration_ms: if self.playback.duration_ms > 0 {
                Some(self.playback.duration_ms)
            } else {
                None
            },
            available: true,
            context: None,
            restricted: false,
            saved: None,
        })
    }
    pub fn apply_track_to_playback(&mut self, item: &BrowseItem) {
        self.playback.track_uri.clone_from(&item.uri);
        self.playback.title.clone_from(&item.title);
        self.playback.artist.clone_from(&item.subtitle);
        self.playback.artists = if item.artists.is_empty() && !item.subtitle.is_empty() {
            vec![(item.subtitle.clone(), None)]
        } else {
            item.artists.clone()
        };
        if let Some((album_name, album_uri)) = &item.album {
            self.playback.album.clone_from(album_name);
            self.playback.album_uri = Some(album_uri.clone());
        } else {
            self.playback.album.clear();
            self.playback.album_uri = None;
        }
        self.playback.artwork_url.clone_from(&item.artwork_url);
        self.playback.duration_ms = item.duration_ms.unwrap_or(0);
        self.playback.progress_ms = 0;
        self.playback.playing = true;
        self.playback.observed_at = Some(Instant::now());
        self.playback.cached_track = false;
    }
}

/// Central action resolver determining context actions for any entity.
#[must_use]
pub fn actions_for(item: &BrowseItem, route: &Route, _state: &AppState) -> Vec<ContextAction> {
    let mut actions = Vec::new();
    match item.kind {
        EntityKind::Track => {
            if item.available && item.uri.is_some() {
                actions.push(ContextAction::Play);
                actions.push(ContextAction::PlayNext);
                actions.push(ContextAction::AddToQueue);
            }
            if item.saved == Some(true) {
                actions.push(ContextAction::Unlike);
            } else {
                actions.push(ContextAction::Like);
            }
            if item.uri.is_some() {
                actions.push(ContextAction::AddToPlaylist);
            }
            if matches!(route, Route::Playlist { .. }) {
                actions.push(ContextAction::RemoveFromPlaylist);
            }
            if item.album.is_some() {
                actions.push(ContextAction::GoToAlbum);
            }
            if item.artists.iter().any(|(_, uri)| uri.is_some()) {
                actions.push(ContextAction::GoToArtist);
            }
            if item.external_url.is_some() || item.uri.is_some() {
                actions.push(ContextAction::OpenInSpotify);
            }
        }
        EntityKind::Album => {
            actions.push(ContextAction::Play);
            actions.push(ContextAction::ShuffleContext);
            if item.saved == Some(true) {
                actions.push(ContextAction::RemoveSavedAlbum);
            } else {
                actions.push(ContextAction::SaveAlbum);
            }
            if item.uri.is_some() {
                actions.push(ContextAction::AddToPlaylist);
            }
            if item.artists.iter().any(|(_, uri)| uri.is_some()) {
                actions.push(ContextAction::GoToArtist);
            }
            if item.external_url.is_some() || item.uri.is_some() {
                actions.push(ContextAction::OpenInSpotify);
            }
        }
        EntityKind::Artist => {
            actions.push(ContextAction::Play);
            if item.saved == Some(true) {
                actions.push(ContextAction::UnfollowArtist);
            } else {
                actions.push(ContextAction::FollowArtist);
            }
            if item.external_url.is_some() || item.uri.is_some() {
                actions.push(ContextAction::OpenInSpotify);
            }
        }
        EntityKind::Playlist => {
            actions.push(ContextAction::Play);
            actions.push(ContextAction::ShuffleContext);
            actions.push(ContextAction::RenamePlaylist);
            if matches!(route, Route::Library) {
                actions.push(ContextAction::DeletePlaylist);
            }
            if item.external_url.is_some() || item.uri.is_some() {
                actions.push(ContextAction::OpenInSpotify);
            }
        }
        EntityKind::Device => {
            actions.push(ContextAction::Play);
        }
        EntityKind::Action => {
            if item.id == "play-context" {
                actions.push(ContextAction::Play);
            } else if item.id == "open-external" {
                actions.push(ContextAction::OpenInSpotify);
            } else if item.id.starts_with("shortcut:") {
                actions.push(ContextAction::EditShortcut);
                actions.push(ContextAction::RemoveShortcutBinding);
                actions.push(ContextAction::ResetShortcutToDefault);
                actions.push(ContextAction::ResetAllShortcutsToDefault);
            }
        }
        EntityKind::Message => {}
    }
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stale_page_response_cannot_replace_new_navigation() {
        let mut state = AppState::default();
        let old = PageState::loading(Route::Home, 0);
        state.navigate(Route::Library);
        assert!(!state.accept_page(old));
        assert_eq!(state.page.route, Route::Library);
    }

    #[test]
    fn list_cursor_scrolls_to_keep_selection_visible() {
        let mut cursor = ListCursor::default();
        cursor.move_to(12, 30, 5);
        assert_eq!(cursor.selected, 12);
        assert_eq!(cursor.offset, 8);
        cursor.move_by(-10, 30, 5);
        assert_eq!(cursor.selected, 2);
        assert_eq!(cursor.offset, 2);
    }

    #[test]
    fn playback_history_records_and_caps_at_fifty() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:initial".into());
        state.playback.title = "Initial Song".into();

        for i in 0..60 {
            state.push_playback_history();
            state.playback.track_uri = Some(format!("spotify:track:{i}"));
            state.playback.title = format!("Song {i}");
        }
        assert_eq!(state.playback_history.len(), 50);
        assert_eq!(state.playback_history.first().unwrap().title, "Song 9");
        assert_eq!(state.playback_history.last().unwrap().title, "Song 58");
    }

    #[test]
    fn apply_track_to_playback_populates_fields() {
        let mut state = AppState::default();
        let item = BrowseItem {
            id: "track1".into(),
            kind: EntityKind::Track,
            title: "Test Track".into(),
            subtitle: "Test Artist".into(),
            metadata: String::new(),
            uri: Some("spotify:track:123".into()),
            external_url: None,
            artwork_url: Some("https://example.com/art.jpg".into()),
            artists: vec![("Test Artist".into(), None)],
            album: Some(("Test Album".into(), "spotify:album:456".into())),
            duration_ms: Some(210_000),
            available: true,
            saved: None,
            context: None,
            restricted: false,
        };
        state.apply_track_to_playback(&item);
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:123"));
        assert_eq!(state.playback.title, "Test Track");
        assert_eq!(state.playback.artist, "Test Artist");
        assert_eq!(state.playback.album, "Test Album");
        assert_eq!(state.playback.album_uri.as_deref(), Some("spotify:album:456"));
        assert_eq!(state.playback.artwork_url.as_deref(), Some("https://example.com/art.jpg"));
        assert_eq!(state.playback.duration_ms, 210_000);
        assert_eq!(state.playback.progress_ms, 0);
        assert!(state.playback.playing);
    }

    #[test]
    fn playback_history_deduplicates_consecutive_entries() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:song1".into());
        state.playback.title = "Song 1".into();

        state.push_playback_history();
        assert_eq!(state.playback_history.len(), 1);

        // Pushing identical URI again should be a no-op
        state.push_playback_history();
        assert_eq!(state.playback_history.len(), 1);

        state.playback.track_uri = Some("spotify:track:song2".into());
        state.playback.title = "Song 2".into();
        state.push_playback_history();
        assert_eq!(state.playback_history.len(), 2);
    }

    #[test]
    fn progress_at_interpolates_when_duration_is_zero() {
        let start = Instant::now();
        let playback = PlaybackState {
            playing: true,
            duration_ms: 0,
            progress_ms: 1_000,
            observed_at: Some(start),
            ..PlaybackState::default()
        };

        let later = start + Duration::from_millis(500);
        assert!(playback.progress_at(later) >= 1_500);
    }

    #[test]
    fn playback_as_browse_item_creates_valid_item() {
        let mut state = AppState::default();
        assert!(state.playback_as_browse_item().is_none());

        state.playback.track_uri = Some("spotify:track:abc".into());
        state.playback.title = "My Song".into();
        state.playback.artist = "My Artist".into();
        state.playback.duration_ms = 180_000;

        let item = state.playback_as_browse_item().unwrap();
        assert_eq!(item.uri.as_deref(), Some("spotify:track:abc"));
        assert_eq!(item.title, "My Song");
        assert_eq!(item.subtitle, "My Artist");
        assert_eq!(item.duration_ms, Some(180_000));
    }

    #[test]
    fn layout_mode_for_size_with_settings_matrix() {
        // Resize
        assert_eq!(LayoutMode::for_size(79, 24), LayoutMode::Resize);
        assert_eq!(LayoutMode::for_size(80, 23), LayoutMode::Resize);

        // SidePlayer: height <= 28, width >= 100
        assert_eq!(LayoutMode::for_size(100, 24), LayoutMode::SidePlayer);
        assert_eq!(LayoutMode::for_size(120, 28), LayoutMode::SidePlayer);
        assert_eq!(LayoutMode::for_size(160, 26), LayoutMode::SidePlayer);

        // Wide: width >= 140 and height > 28
        assert_eq!(LayoutMode::for_size(140, 30), LayoutMode::Wide);
        assert_eq!(LayoutMode::for_size(160, 45), LayoutMode::Wide);

        // StackedQueue: height >= 38 and width < 140
        assert_eq!(LayoutMode::for_size(90, 40), LayoutMode::StackedQueue);
        assert_eq!(LayoutMode::for_size(120, 38), LayoutMode::StackedQueue);

        // Compact: width < 100 and height > 28 and height < 38
        assert_eq!(LayoutMode::for_size(90, 30), LayoutMode::Compact);

        // Standard: 100 <= width < 140 and 28 < height < 38
        assert_eq!(LayoutMode::for_size(110, 30), LayoutMode::Standard);
        assert_eq!(LayoutMode::for_size(130, 35), LayoutMode::Standard);

        // Disabled SidePlayer (max_height = 0)
        assert_eq!(
            LayoutMode::for_size_with_settings(110, 26, 0, 100, 140, 38),
            LayoutMode::Standard
        );

        // Disabled StackedQueue (min_height = 999)
        assert_eq!(
            LayoutMode::for_size_with_settings(90, 45, 28, 100, 140, 999),
            LayoutMode::Compact
        );
    }

    #[test]
    fn layout_inline_queue_and_queue_visible_rules() {
        // Standard: no inline queue
        let mut state =
            AppState { layout: LayoutMode::Standard, wide_queue: true, ..Default::default() };
        assert!(!state.layout_has_inline_queue());
        assert!(!state.queue_visible());

        // SidePlayer: no inline queue
        state.layout = LayoutMode::SidePlayer;
        assert!(!state.layout_has_inline_queue());
        assert!(!state.queue_visible());

        // Wide: has inline queue when wide_queue is true and not on Route::Queue
        state.layout = LayoutMode::Wide;
        assert!(state.layout_has_inline_queue());
        assert!(state.queue_visible());
        state.page.route = Route::Queue;
        assert!(state.layout_has_inline_queue());
        assert!(!state.queue_visible());

        // StackedQueue: has inline queue when wide_queue is true and not on Route::Queue
        state.layout = LayoutMode::StackedQueue;
        state.page.route = Route::Home;
        assert!(state.layout_has_inline_queue());
        assert!(state.queue_visible());
        state.wide_queue = false;
        assert!(!state.layout_has_inline_queue());
        assert!(!state.queue_visible());
    }

    #[test]
    fn navigate_and_back_synchronizes_primary_section_for_queue() {
        let mut state = AppState::default();
        assert_eq!(state.primary_section, Route::Home);

        state.navigate(Route::Queue);
        assert_eq!(state.primary_section, Route::Queue);

        state.navigate(Route::Library);
        assert_eq!(state.primary_section, Route::Library);

        state.back();
        assert_eq!(state.page.route, Route::Queue);
        assert_eq!(state.primary_section, Route::Queue);

        state.back();
        assert_eq!(state.page.route, Route::Home);
        assert_eq!(state.primary_section, Route::Home);
    }

    #[test]
    fn visible_focuses_excludes_sidebar_in_side_player_and_stacked_queue() {
        let mut state = AppState { layout: LayoutMode::SidePlayer, ..Default::default() };
        assert!(!state.visible_focuses().contains(&FocusRegion::Sidebar));

        state.layout = LayoutMode::StackedQueue;
        assert!(!state.visible_focuses().contains(&FocusRegion::Sidebar));

        state.layout = LayoutMode::Standard;
        assert!(state.visible_focuses().contains(&FocusRegion::Sidebar));
    }

    #[test]
    fn display_device_name_and_target_device_id_fallbacks() {
        let mut state = AppState::default();
        assert_eq!(state.display_device_name(), "choose with d");
        assert_eq!(state.target_device_id(), None);

        // Local player ready
        state.local_device_id = Some("local-1".into());
        assert_eq!(state.display_device_name(), "lspotify");
        assert_eq!(state.target_device_id().as_deref(), Some("local-1"));

        // Remote playback device known
        state.playback.device_id = Some("remote-1".into());
        state.playback.device_name = Some("Living Room".into());
        assert_eq!(state.display_device_name(), "Living Room");

        // Selected device wins target_device_id
        state.selected_device_id = Some("selected-1".into());
        assert_eq!(state.target_device_id().as_deref(), Some("selected-1"));
    }

    #[test]
    fn library_tab_cycling() {
        let tab = LibraryTab::LikedSongs;
        assert_eq!(tab.next(), LibraryTab::Albums);
        assert_eq!(tab.next().next(), LibraryTab::Playlists);
        assert_eq!(tab.next().next().next(), LibraryTab::LikedSongs);

        assert_eq!(tab.previous(), LibraryTab::Playlists);
        assert_eq!(tab.previous().previous(), LibraryTab::Albums);
        assert_eq!(tab.previous().previous().previous(), LibraryTab::LikedSongs);
    }

    #[test]
    fn actions_for_tracks() {
        let state = AppState::default();
        let track = BrowseItem {
            id: "t1".into(),
            kind: EntityKind::Track,
            title: "Track 1".into(),
            subtitle: "Artist 1".into(),
            metadata: String::new(),
            uri: Some("spotify:track:1".into()),
            external_url: None,
            artwork_url: None,
            artists: vec![("Artist 1".into(), Some("spotify:artist:1".into()))],
            album: Some(("Album 1".into(), "spotify:album:1".into())),
            duration_ms: Some(180_000),
            available: true,
            saved: Some(true),
            context: None,
            restricted: false,
        };

        // Liked track in home route
        let actions = actions_for(&track, &Route::Home, &state);
        assert!(actions.contains(&ContextAction::Unlike));
        assert!(!actions.contains(&ContextAction::Like));
        assert!(actions.contains(&ContextAction::Play));
        assert!(actions.contains(&ContextAction::AddToQueue));
        assert!(actions.contains(&ContextAction::AddToPlaylist));
        assert!(actions.contains(&ContextAction::GoToAlbum));
        assert!(actions.contains(&ContextAction::GoToArtist));
        assert!(!actions.contains(&ContextAction::RemoveFromPlaylist));

        // Track in Route::Playlist
        let actions_in_playlist = actions_for(
            &track,
            &Route::Playlist { uri: "spotify:playlist:1".into(), title: "Mix".into() },
            &state,
        );
        assert!(actions_in_playlist.contains(&ContextAction::RemoveFromPlaylist));
    }

    #[test]
    fn actions_for_albums_artists_and_playlists() {
        let state = AppState::default();
        let album = BrowseItem {
            id: "a1".into(),
            kind: EntityKind::Album,
            title: "Album 1".into(),
            subtitle: "Artist 1".into(),
            metadata: String::new(),
            uri: Some("spotify:album:1".into()),
            external_url: None,
            artwork_url: None,
            artists: vec![("Artist 1".into(), Some("spotify:artist:1".into()))],
            album: None,
            duration_ms: None,
            available: true,
            saved: Some(false),
            context: None,
            restricted: false,
        };
        let album_actions = actions_for(&album, &Route::Home, &state);
        assert!(album_actions.contains(&ContextAction::Play));
        assert!(album_actions.contains(&ContextAction::ShuffleContext));
        assert!(album_actions.contains(&ContextAction::SaveAlbum));
        assert!(!album_actions.contains(&ContextAction::RemoveSavedAlbum));
        assert!(album_actions.contains(&ContextAction::GoToArtist));

        let artist = BrowseItem {
            id: "ar1".into(),
            kind: EntityKind::Artist,
            title: "Artist 1".into(),
            subtitle: String::new(),
            metadata: String::new(),
            uri: Some("spotify:artist:1".into()),
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: None,
            available: true,
            saved: Some(true),
            context: None,
            restricted: false,
        };
        let artist_actions = actions_for(&artist, &Route::Home, &state);
        assert!(artist_actions.contains(&ContextAction::Play));
        assert!(artist_actions.contains(&ContextAction::UnfollowArtist));
        assert!(!artist_actions.contains(&ContextAction::FollowArtist));

        let playlist = BrowseItem {
            id: "p1".into(),
            kind: EntityKind::Playlist,
            title: "My Playlist".into(),
            subtitle: String::new(),
            metadata: String::new(),
            uri: Some("spotify:playlist:1".into()),
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: None,
            available: true,
            saved: None,
            context: None,
            restricted: false,
        };
        let playlist_actions = actions_for(&playlist, &Route::Home, &state);
        assert!(playlist_actions.contains(&ContextAction::Play));
        assert!(playlist_actions.contains(&ContextAction::ShuffleContext));
        assert!(playlist_actions.contains(&ContextAction::RenamePlaylist));
        assert!(!playlist_actions.contains(&ContextAction::DeletePlaylist));

        let playlist_actions_in_library = actions_for(&playlist, &Route::Library, &state);
        assert!(playlist_actions_in_library.contains(&ContextAction::DeletePlaylist));
    }

    #[test]
    fn fresh_launch_home_response_selects_row_zero() {
        let mut state = AppState::default();
        assert!(state.startup_focus_pending);
        let mut home_page = PageState::loading(Route::Home, 0);
        home_page.state = LoadState::Ready;
        home_page.sections = vec![Section {
            title: "Recently played".into(),
            items: vec![
                BrowseItem::message("item-1", "Track 1"),
                BrowseItem::message("item-2", "Track 2"),
            ],
        }];
        assert!(state.accept_page(home_page));
        assert_eq!(state.focus, FocusRegion::Content);
        assert_eq!(state.page.cursor.selected, 0);
        assert_eq!(state.page.cursor.offset, 0);
        assert!(!state.startup_focus_pending);
    }

    #[test]
    fn user_interaction_before_home_response_prevents_automatic_focus_changes() {
        let mut state = AppState::default();
        assert!(state.startup_focus_pending);
        state.focus = FocusRegion::Sidebar;
        state.startup_focus_pending = false;

        let mut home_page = PageState::loading(Route::Home, 0);
        home_page.state = LoadState::Ready;
        home_page.sections = vec![Section {
            title: "Recently played".into(),
            items: vec![
                BrowseItem::message("item-1", "Track 1"),
                BrowseItem::message("item-2", "Track 2"),
            ],
        }];
        assert!(state.accept_page(home_page));
        assert_eq!(state.focus, FocusRegion::Sidebar);
        assert!(!state.startup_focus_pending);
    }

    #[test]
    fn subsequent_home_refreshes_do_not_reset_cursor() {
        let mut state = AppState::default();
        let mut home_page = PageState::loading(Route::Home, 0);
        home_page.state = LoadState::Ready;
        home_page.sections = vec![Section {
            title: "Recently played".into(),
            items: vec![
                BrowseItem::message("item-1", "Track 1"),
                BrowseItem::message("item-2", "Track 2"),
                BrowseItem::message("item-3", "Track 3"),
            ],
        }];
        assert!(state.accept_page(home_page));
        state.content_height = 2;
        state.page.cursor.selected = 2;
        state.page.cursor.offset = 1;

        let mut refreshed_home = PageState::loading(Route::Home, state.generation);
        refreshed_home.state = LoadState::Ready;
        refreshed_home.sections = vec![Section {
            title: "Recently played".into(),
            items: vec![
                BrowseItem::message("item-1", "Track 1"),
                BrowseItem::message("item-2", "Track 2"),
                BrowseItem::message("item-3", "Track 3"),
                BrowseItem::message("item-4", "Track 4"),
            ],
        }];
        assert!(state.accept_page(refreshed_home));
        assert_eq!(state.page.cursor.selected, 2);
        assert_eq!(state.page.cursor.offset, 1);
    }
}
