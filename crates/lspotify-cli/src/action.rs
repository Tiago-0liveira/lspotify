use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use lspotify_core::LocalPlaybackBackendPreference;

use crate::shortcuts::{KeyBinding, ShortcutAction};
use crate::state::{
    AppState, BrowseItem, ContextAction, EntityKind, FocusRegion, LibraryTab, Notice, NoticeKind,
    Overlay, PageState, Route, SearchFilter, Section, SettingsTab, actions_for,
};

pub const LOCAL_DEVICE_PLACEHOLDER: &str = "lspotify-local";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Effect {
    LoadPage { route: Route, generation: u64, offset: u32, query: String, tab: Option<LibraryTab> },
    RefreshPlayback,
    RefreshQueue,
    TogglePlayback,
    Previous,
    Next,
    Seek(i64),
    Volume(u8),
    Shuffle(bool),
    Repeat(u8),
    PlayTrack { uri: String, device_id: Option<String> },
    PlayContext { uri: String, position: usize, device_id: Option<String> },
    Enqueue(String),
    Transfer(String),
    TransferLocal(String),
    StartLocalPlayback,
    ChangeLocalPlaybackBackend(LocalPlaybackBackendPreference),
    OpenExternal(String),
    SaveSettings,
    SetSaved { uri: String, saved: bool },
    AddToPlaylist { playlist_id: String, track_uri: String },
    RemoveFromPlaylist { playlist_uri: String, track_uri: String },
    RenamePlaylist { playlist_id: String, new_name: String },
    LoadPlaylistsForPicker { pending_uri: String },
    WarmLikedSongs,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Navigate(Route),
    Back,
    FocusNext(bool),
    Move(isize),
    Page(isize),
    Home,
    End,
    LocalLeft,
    LocalRight,
    Activate,
    OpenActions,
    ToggleSaved,
    Inspect,
    ToggleHelp,
    StartSearch,
    StartFilter,
    Insert(char),
    Backspace,
    SubmitText,
    Cancel,
    TogglePlayback,
    Previous,
    Next,
    Seek(i64),
    Volume(i8),
    Shuffle,
    Repeat,
    Enqueue,
    Refresh,
    PlayDetailContext,
    PlayDetailContextWithShuffle,
    OpenDetailSpotify,
    Quit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HitTarget {
    Sidebar(usize),
    TopNav(Route),
    SettingsTab(SettingsTab),
    PlayerArtist { title: String, uri: String },
    PlayerAlbum { title: String, uri: String },
    PlayerDevice,
    MenuButton,
    ContentRow(usize),
    QueueRow(usize),
    ActionRow(usize),
    PlaylistPickerRow(usize),
    PlayerToggle,
    PlayerPrevious,
    PlayerNext,
    PlayerProgress,
    PlayerVolume,
    PlayerShuffle,
    PlayerRepeat,
    SearchField,
    Help,
    DetailPlay,
    DetailShuffle,
    DetailOpenSpotify,
}

#[derive(Clone, Debug)]
struct HitRegion {
    rect: Rect,
    target: HitTarget,
}

#[derive(Clone, Debug, Default)]
pub struct HitMap {
    regions: Vec<HitRegion>,
    pub content: Option<Rect>,
    pub queue: Option<Rect>,
    pub sidebar: Option<Rect>,
    last_click: Option<(HitTarget, Instant)>,
}

impl HitMap {
    pub fn clear(&mut self) {
        self.regions.clear();
        self.content = None;
        self.queue = None;
        self.sidebar = None;
    }
    pub fn add(&mut self, rect: Rect, target: HitTarget) {
        self.regions.push(HitRegion { rect, target });
    }
    pub fn target_at(&self, column: u16, row: u16) -> Option<HitTarget> {
        self.regions
            .iter()
            .rev()
            .find(|hit| hit.rect.contains((column, row).into()))
            .map(|hit| hit.target.clone())
    }
    pub(crate) fn rect_for(&self, target: &HitTarget) -> Option<Rect> {
        self.regions.iter().rev().find(|hit| &hit.target == target).map(|hit| hit.rect)
    }
}

fn handle_shortcut_capture_key(
    state: &mut AppState,
    action: ShortcutAction,
    conflict: Option<(KeyBinding, ShortcutAction)>,
    key: KeyEvent,
) -> Vec<Effect> {
    if let Some((new_binding, _)) = conflict {
        if state.shortcuts.matches_action(ShortcutAction::Activate, &key) {
            state.shortcuts.force_assign(action, new_binding);
            state.overlay = None;
            refresh_settings_page(state);
            vec![Effect::SaveSettings]
        } else if state.shortcuts.matches_action(ShortcutAction::Cancel, &key) {
            state.overlay = None;
            Vec::new()
        } else {
            Vec::new()
        }
    } else if state.shortcuts.matches_action(ShortcutAction::Cancel, &key) {
        state.overlay = None;
        Vec::new()
    } else {
        match key.code {
            KeyCode::Delete => {
                state.shortcuts.remove_binding(action);
                state.overlay = None;
                refresh_settings_page(state);
                vec![Effect::SaveSettings]
            }
            KeyCode::Modifier(_) => Vec::new(),
            _ => {
                let new_binding = KeyBinding::from_event(&key);
                if state
                    .shortcuts
                    .get_bindings(action)
                    .iter()
                    .any(|b| b.conflicts_with(&new_binding))
                {
                    state.overlay = None;
                    return Vec::new();
                }
                if let Some(conflicting) = state.shortcuts.find_conflict(action, &new_binding) {
                    state.overlay = Some(Overlay::ShortcutCapture {
                        action,
                        conflict: Some((new_binding, conflicting)),
                    });
                    Vec::new()
                } else {
                    state.shortcuts.force_assign(action, new_binding);
                    state.overlay = None;
                    refresh_settings_page(state);
                    vec![Effect::SaveSettings]
                }
            }
        }
    }
}

/// True while keystrokes feed a text field (search box, help filter, playlist rename).
fn is_typing(state: &AppState) -> bool {
    state.text_entry
        || matches!(
            state.overlay,
            Some(Overlay::Help { editing: true, .. } | Overlay::RenamePlaylist { .. })
        )
}

pub fn dispatch_key(state: &mut AppState, key: KeyEvent) -> Vec<Effect> {
    state.startup_focus_pending = false;
    // Exit precedes shortcut capture, text entry, and overlays. While typing, printable quit
    // keys (`x`, `Shift+Q`) are text; only Ctrl-modified quit keys still exit.
    let typing = is_typing(state);
    let printable =
        matches!(key.code, KeyCode::Char(_)) && !key.modifiers.contains(KeyModifiers::CONTROL);
    if state.shortcuts.is_quit(&key) && !(typing && printable) {
        return reduce(state, Action::Quit);
    }
    if let Some(Overlay::ShortcutCapture { action, conflict }) = state.overlay.clone() {
        return handle_shortcut_capture_key(state, action, conflict, key);
    }
    let action = if typing {
        if key.code == KeyCode::Backspace {
            Action::Backspace
        } else if state.shortcuts.matches_action(ShortcutAction::Cancel, &key) {
            Action::Cancel
        } else if state.shortcuts.matches_action(ShortcutAction::Activate, &key) {
            Action::SubmitText
        } else {
            match key.code {
                KeyCode::Down if state.overlay.is_none() => {
                    state.text_entry = false;
                    state.focus = FocusRegion::Content;
                    if state.page.route == Route::Search && state.search_query.is_empty() {
                        state.populate_recent_searches();
                    }
                    return Vec::new();
                }
                KeyCode::Tab if state.overlay.is_none() => {
                    state.text_entry = false;
                    if state.page.route == Route::Search && state.search_query.is_empty() {
                        state.populate_recent_searches();
                    }
                    Action::FocusNext(key.modifiers.contains(KeyModifiers::SHIFT))
                }
                KeyCode::Char(ch) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    Action::Insert(ch)
                }
                _ => return Vec::new(),
            }
        }
    } else {
        let resolved = state.shortcuts.resolve(&key, &state.page.route, has_detail_actions(state));
        if matches!(resolved, Some(ShortcutAction::MoveUp))
            && state.page.route == Route::Search
            && state.focus == FocusRegion::Content
            && state.page.cursor.selected == 0
        {
            state.text_entry = true;
            if state.search_query.is_empty() {
                state.populate_recent_searches();
            }
            return Vec::new();
        }
        let Some(shortcut) = resolved else {
            return Vec::new();
        };
        match shortcut {
            ShortcutAction::Quit => Action::Quit,
            ShortcutAction::Home => Action::Navigate(Route::Home),
            ShortcutAction::Search => Action::Navigate(Route::Search),
            ShortcutAction::Library => Action::Navigate(Route::Library),
            ShortcutAction::Settings => Action::Navigate(Route::Settings),
            ShortcutAction::Queue => Action::Navigate(Route::Queue),
            ShortcutAction::Devices => Action::Navigate(Route::Devices),
            ShortcutAction::FocusNext => Action::FocusNext(false),
            ShortcutAction::FocusPrevious => Action::FocusNext(true),
            ShortcutAction::MoveUp => Action::Move(-1),
            ShortcutAction::MoveDown => Action::Move(1),
            ShortcutAction::PageUp => Action::Page(-1),
            ShortcutAction::PageDown => Action::Page(1),
            ShortcutAction::CursorHome => Action::Home,
            ShortcutAction::CursorEnd => Action::End,
            ShortcutAction::PreviousTab => Action::LocalLeft,
            ShortcutAction::NextTab => Action::LocalRight,
            ShortcutAction::Activate => Action::Activate,
            ShortcutAction::Cancel => Action::Cancel,
            ShortcutAction::StartSearch => Action::StartSearch,
            ShortcutAction::StartFilter => Action::StartFilter,
            ShortcutAction::TogglePlayback => Action::TogglePlayback,
            ShortcutAction::PreviousTrack => Action::Previous,
            ShortcutAction::NextTrack => Action::Next,
            ShortcutAction::SeekBackward => Action::Seek(-5_000),
            ShortcutAction::SeekForward => Action::Seek(5_000),
            ShortcutAction::VolumeDown => Action::Volume(-5),
            ShortcutAction::VolumeUp => Action::Volume(5),
            ShortcutAction::Shuffle => Action::Shuffle,
            ShortcutAction::Repeat => Action::Repeat,
            ShortcutAction::AddToQueue => Action::Enqueue,
            ShortcutAction::ToggleSaved => Action::ToggleSaved,
            ShortcutAction::OpenActions => Action::OpenActions,
            ShortcutAction::DetailPlay => Action::PlayDetailContext,
            ShortcutAction::DetailShuffle => Action::PlayDetailContextWithShuffle,
            ShortcutAction::DetailOpenSpotify => Action::OpenDetailSpotify,
            ShortcutAction::ToggleHelp => Action::ToggleHelp,
            ShortcutAction::Inspect => Action::Inspect,
        }
    };
    reduce(state, action)
}

pub fn dispatch_mouse(
    state: &mut AppState,
    hits: &mut HitMap,
    event: MouseEvent,
    now: Instant,
) -> Vec<Effect> {
    state.startup_focus_pending = false;
    if !state.mouse {
        return Vec::new();
    }
    let target = hits.target_at(event.column, event.row);
    match event.kind {
        MouseEventKind::ScrollUp => {
            state.focus = hovered_focus(hits, event.column, event.row).unwrap_or(state.focus);
            reduce(state, Action::Move(-3))
        }
        MouseEventKind::ScrollDown => {
            state.focus = hovered_focus(hits, event.column, event.row).unwrap_or(state.focus);
            reduce(state, Action::Move(3))
        }
        MouseEventKind::Down(MouseButton::Right) => {
            select_target(state, target.as_ref());
            reduce(state, Action::OpenActions)
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let double = target.as_ref().is_some_and(|target| {
                hits.last_click.as_ref().is_some_and(|(last, at)| {
                    last == target
                        && now.saturating_duration_since(*at) <= Duration::from_millis(450)
                })
            });
            if let Some(target) = target.clone() {
                hits.last_click = Some((target, now));
            }
            let direct = match target.clone() {
                Some(HitTarget::MenuButton) => {
                    state.overlay = Some(Overlay::Menu);
                    return Vec::new();
                }
                Some(HitTarget::PlayerToggle) => Some(Action::TogglePlayback),
                Some(HitTarget::TopNav(route)) => Some(Action::Navigate(route)),
                Some(HitTarget::SettingsTab(tab)) => {
                    switch_settings_tab(state, tab);
                    return Vec::new();
                }
                Some(HitTarget::PlayerArtist { title, uri }) => {
                    Some(Action::Navigate(Route::Artist { title, uri }))
                }
                Some(HitTarget::PlayerAlbum { title, uri }) => {
                    Some(Action::Navigate(Route::Album { title, uri }))
                }
                Some(HitTarget::PlayerDevice) => Some(Action::Navigate(Route::Devices)),
                Some(HitTarget::PlayerPrevious) => Some(Action::Previous),
                Some(HitTarget::PlayerNext) => Some(Action::Next),
                Some(HitTarget::PlayerProgress) => {
                    let desired = hits.rect_for(&HitTarget::PlayerProgress).and_then(|area| {
                        seek_position_at(area, event.column, state.playback.duration_ms)
                    });
                    desired.map(|desired| {
                        let current = state.playback.progress_at(now);
                        Action::Seek(
                            i64::try_from(desired)
                                .unwrap_or(i64::MAX)
                                .saturating_sub(i64::try_from(current).unwrap_or(i64::MAX)),
                        )
                    })
                }
                Some(HitTarget::PlayerVolume) => {
                    let desired = hits.rect_for(&HitTarget::PlayerVolume).map_or(
                        state.playback.volume,
                        |area| {
                            let column = event.column.saturating_sub(area.x).min(area.width);
                            u8::try_from(u32::from(column) * 100 / u32::from(area.width.max(1)))
                                .unwrap_or(100)
                        },
                    );
                    Some(Action::Volume(
                        i8::try_from(desired)
                            .unwrap_or(100)
                            .saturating_sub(i8::try_from(state.playback.volume).unwrap_or(100)),
                    ))
                }
                Some(HitTarget::PlayerShuffle) => Some(Action::Shuffle),
                Some(HitTarget::PlayerRepeat) => Some(Action::Repeat),
                Some(HitTarget::SearchField) => Some(Action::StartSearch),
                Some(HitTarget::Help) => Some(Action::ToggleHelp),
                Some(HitTarget::DetailPlay) => Some(Action::PlayDetailContext),
                Some(HitTarget::DetailShuffle) => Some(Action::PlayDetailContextWithShuffle),
                Some(HitTarget::DetailOpenSpotify) => Some(Action::OpenDetailSpotify),
                _ => None,
            };
            if let Some(action) = direct {
                return reduce(state, action);
            }
            select_target(state, target.as_ref());
            if matches!(target, Some(HitTarget::Sidebar(_))) {
                return reduce(state, Action::Activate);
            }
            if double { reduce(state, Action::Activate) } else { Vec::new() }
        }
        _ => Vec::new(),
    }
}

fn hovered_focus(hits: &HitMap, column: u16, row: u16) -> Option<FocusRegion> {
    if hits.queue.is_some_and(|area| area.contains((column, row).into())) {
        Some(FocusRegion::Queue)
    } else if hits.sidebar.is_some_and(|area| area.contains((column, row).into())) {
        Some(FocusRegion::Sidebar)
    } else if hits.content.is_some_and(|area| area.contains((column, row).into())) {
        Some(FocusRegion::Content)
    } else {
        None
    }
}

/// Maps a terminal pointer column to a playback position using the actual progress hit rectangle.
/// The last visible cell maps to the end of the track and invalid durations cannot be sought.
fn seek_position_at(track: Rect, column: u16, duration_ms: u64) -> Option<u64> {
    if duration_ms == 0 || track.width == 0 {
        return None;
    }
    let offset = column.saturating_sub(track.x).min(track.width.saturating_sub(1));
    let denominator = u64::from(track.width.saturating_sub(1).max(1));
    Some(duration_ms.saturating_mul(u64::from(offset)) / denominator)
}
fn select_target(state: &mut AppState, target: Option<&HitTarget>) {
    match target {
        Some(HitTarget::Sidebar(index)) => {
            state.focus = FocusRegion::Sidebar;
            state.sidebar.selected = *index;
        }
        Some(HitTarget::ContentRow(index)) => {
            state.focus = FocusRegion::Content;
            state.page.cursor.selected = *index;
        }
        Some(HitTarget::QueueRow(index)) => {
            state.focus = FocusRegion::Queue;
            state.queue.selected = *index;
        }
        Some(HitTarget::ActionRow(index)) => {
            if let Some(Overlay::Actions { selected, .. }) = &mut state.overlay {
                *selected = *index;
            }
        }
        Some(HitTarget::PlaylistPickerRow(index)) => {
            if let Some(Overlay::PlaylistPicker { selected, .. }) = &mut state.overlay {
                *selected = *index;
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_lines)]
pub fn reduce(state: &mut AppState, action: Action) -> Vec<Effect> {
    match action {
        Action::Navigate(route) => load_route(state, route),
        Action::Back => {
            state.back();
            Vec::new()
        }
        Action::FocusNext(reverse) => {
            state.cycle_focus(reverse);
            Vec::new()
        }
        Action::Move(delta) => {
            match &mut state.overlay {
                Some(Overlay::Actions { selected, actions }) => {
                    *selected =
                        selected.saturating_add_signed(delta).min(actions.len().saturating_sub(1));
                }
                Some(Overlay::PlaylistPicker { selected, playlists, .. }) => {
                    *selected = selected
                        .saturating_add_signed(delta)
                        .min(playlists.len().saturating_sub(1));
                }
                Some(Overlay::Menu) => state.sidebar.move_by(delta, AppState::NAVIGATION.len(), 6),
                _ => move_selection(state, delta),
            }
            Vec::new()
        }
        Action::Page(direction) => {
            let step = isize::try_from(state.content_height.max(1)).unwrap_or(isize::MAX);
            move_selection(state, step.saturating_mul(direction));
            Vec::new()
        }
        Action::Home => {
            move_to_edge(state, false);
            Vec::new()
        }
        Action::End => {
            move_to_edge(state, true);
            Vec::new()
        }
        Action::LocalLeft => cycle_local(state, false),
        Action::LocalRight => cycle_local(state, true),
        Action::Activate => {
            if let Some(Overlay::Actions { selected, actions }) = state.overlay.clone() {
                activate_action_menu(state, selected, &actions)
            } else if let Some(Overlay::PlaylistPicker { selected, playlists, pending_uri }) =
                state.overlay.clone()
            {
                activate_playlist_picker(state, selected, &playlists, &pending_uri)
            } else {
                activate(state)
            }
        }
        Action::OpenActions => {
            if state.layout == crate::state::LayoutMode::Compact
                && state.focus != FocusRegion::Content
            {
                state.overlay = Some(Overlay::Menu);
            } else if let Some(item) = state.selected_item() {
                let actions = actions_for(item, &state.page.route, state);
                if !actions.is_empty() {
                    state.overlay = Some(Overlay::Actions { selected: 0, actions });
                }
            }
            Vec::new()
        }
        Action::ToggleSaved => toggle_saved(state),
        Action::Inspect => {
            if state.selected_item().is_some() {
                state.overlay = Some(Overlay::Inspector);
            }
            Vec::new()
        }
        Action::ToggleHelp => {
            state.overlay = if matches!(state.overlay, Some(Overlay::Help { .. })) {
                None
            } else {
                Some(Overlay::Help { query: String::new(), editing: false })
            };
            Vec::new()
        }
        Action::StartSearch => {
            if let Some(Overlay::Help { editing, .. }) = &mut state.overlay {
                *editing = true;
                return Vec::new();
            }
            if state.page.route != Route::Search {
                let effects = load_route(state, Route::Search);
                state.text_entry = true;
                return effects;
            }
            state.text_entry = true;
            Vec::new()
        }
        Action::StartFilter => {
            state.text_entry = true;
            Vec::new()
        }
        Action::Insert(ch) => {
            insert_text(state, ch);
            Vec::new()
        }
        Action::Backspace => {
            backspace(state);
            Vec::new()
        }
        Action::SubmitText => submit_text(state),
        Action::Cancel => {
            state.backend_error = None;
            if state.overlay.is_some() {
                state.overlay = None;
                state.clear_error_notice();
            } else if state.text_entry {
                state.text_entry = false;
                if state.page.route == Route::Search && state.search_query.is_empty() {
                    state.populate_recent_searches();
                }
            } else if state.page.route == Route::Search && !state.search_query.is_empty() {
                state.search_query.clear();
                state.populate_recent_searches();
            } else {
                state.back();
            }
            Vec::new()
        }
        Action::TogglePlayback if state.playback.cached_track && !state.playback.playing => {
            state.playback.track_uri.clone().map_or_else(Vec::new, |uri| {
                vec![Effect::PlayTrack { uri, device_id: state.target_device_id() }]
            })
        }
        Action::TogglePlayback => vec![Effect::TogglePlayback],
        Action::Previous => {
            let current_pos = state.playback.progress_at(Instant::now());
            if current_pos > 3_000 {
                state.playback.progress_ms = 0;
                state.playback.observed_at = Some(Instant::now());
            } else if let Some(previous_playback) = state.playback_history.pop() {
                if let Some(current_item) = state.playback_as_browse_item() {
                    state.queue_upcoming.insert(0, current_item);
                }
                state.playback = previous_playback;
                state.playback.progress_ms = 0;
                state.playback.playing = true;
                state.playback.observed_at = Some(Instant::now());
                state.playback.cached_track = false;
                state.queue_now = state.playback_as_browse_item();
            } else {
                state.playback.progress_ms = 0;
                state.playback.observed_at = Some(Instant::now());
            }
            vec![Effect::Previous]
        }
        Action::Next => {
            if !state.queue_upcoming.is_empty() {
                state.push_playback_history();
                let next_track = state.queue_upcoming.remove(0);
                state.queue.selected =
                    state.queue.selected.min(state.queue_upcoming.len().saturating_sub(1));
                state.apply_track_to_playback(&next_track);
                state.queue_now = Some(next_track);
            }
            vec![Effect::Next]
        }
        Action::Seek(offset) => vec![Effect::Seek(offset)],
        Action::Volume(delta) => {
            state.playback.volume = state.playback.volume.saturating_add_signed(delta).min(100);
            vec![Effect::Volume(state.playback.volume)]
        }
        Action::Shuffle => {
            state.playback.shuffle = !state.playback.shuffle;
            vec![Effect::Shuffle(state.playback.shuffle)]
        }
        Action::Repeat => {
            state.playback.repeat = (state.playback.repeat + 1) % 3;
            vec![Effect::Repeat(state.playback.repeat)]
        }
        Action::Enqueue => enqueue_selected(state),
        Action::Refresh => {
            if state.page.route == Route::Settings {
                refresh_settings_page(state);
                Vec::new()
            } else {
                vec![Effect::LoadPage {
                    route: state.page.route.clone(),
                    generation: state.generation,
                    offset: 0,
                    query: state.search_query.clone(),
                    tab: if state.page.route == Route::Library {
                        Some(state.page.library_tab)
                    } else {
                        None
                    },
                }]
            }
        }
        Action::PlayDetailContext => detail_context_uri(state).map_or_else(Vec::new, |uri| {
            vec![Effect::PlayTrack { uri, device_id: state.target_device_id() }]
        }),
        Action::PlayDetailContextWithShuffle => {
            detail_context_uri(state).map_or_else(Vec::new, |uri| {
                state.playback.shuffle = true;
                vec![
                    Effect::PlayTrack { uri, device_id: state.target_device_id() },
                    Effect::Shuffle(true),
                ]
            })
        }
        Action::OpenDetailSpotify => {
            detail_external_url(state).map_or_else(Vec::new, |url| vec![Effect::OpenExternal(url)])
        }
        Action::Quit => {
            state.quit = true;
            Vec::new()
        }
    }
}

fn has_detail_actions(state: &AppState) -> bool {
    matches!(state.page.route, Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. })
        && state.overlay.is_none()
        && !state.text_entry
}

fn detail_context_uri(state: &AppState) -> Option<String> {
    state.page.uri.clone().or_else(|| match &state.page.route {
        Route::Album { uri, .. } | Route::Playlist { uri, .. } | Route::Artist { uri, .. } => {
            Some(uri.clone())
        }
        _ => None,
    })
}

fn detail_external_url(state: &AppState) -> Option<String> {
    state.page.external_url.clone().or_else(|| {
        detail_context_uri(state).and_then(|uri| {
            uri.parse::<lspotify_core::ids::SpotifyUri>().ok().map(|parsed| parsed.web_url())
        })
    })
}

fn load_route(state: &mut AppState, route: Route) -> Vec<Effect> {
    let is_search = route == Route::Search;
    let is_search_empty = is_search && state.search_query.trim().is_empty();
    let cache_key = match route {
        Route::Library => match state.library_tab {
            LibraryTab::LikedSongs => "library:liked".to_string(),
            LibraryTab::Albums => "library:albums".to_string(),
            LibraryTab::Playlists => "library:playlists".to_string(),
        },
        _ => route.key(),
    };
    let is_fresh = state
        .page_cache
        .get(&cache_key)
        .is_some_and(|(_, cached_at)| cached_at.elapsed() < Duration::from_secs(60));

    let generation = state.navigate(route.clone());
    if is_search {
        state.text_entry = true;
    }

    if is_search_empty {
        state.populate_recent_searches();
        return Vec::new();
    }

    if is_fresh && route != Route::Queue && route != Route::Devices {
        return Vec::new();
    }

    if route == Route::Settings {
        refresh_settings_page(state);
        return Vec::new();
    }

    let is_library = route == Route::Library;
    vec![Effect::LoadPage {
        route,
        generation,
        offset: 0,
        query: state.search_query.clone(),
        tab: if is_library { Some(state.library_tab) } else { None },
    }]
}

fn move_selection(state: &mut AppState, delta: isize) {
    match state.focus {
        FocusRegion::Sidebar => state.sidebar.move_by(delta, AppState::NAVIGATION.len(), 6),
        FocusRegion::Content => {
            let len = state.page.flattened().len();
            state.page.cursor.move_by(delta, len, state.content_height);
        }
        FocusRegion::Queue => {
            state.queue.move_by(delta, state.queue_upcoming.len(), state.queue_height);
        }
        FocusRegion::Player => {}
    }
}

fn move_to_edge(state: &mut AppState, end: bool) {
    let content_len = state.page.flattened().len();
    let (cursor, len, height) = match state.focus {
        FocusRegion::Sidebar => (&mut state.sidebar, AppState::NAVIGATION.len(), 6),
        FocusRegion::Queue => (&mut state.queue, state.queue_upcoming.len(), state.queue_height),
        FocusRegion::Content => (&mut state.page.cursor, content_len, state.content_height),
        FocusRegion::Player => return,
    };
    cursor.move_to(if end { len.saturating_sub(1) } else { 0 }, len, height);
}

fn cycle_local(state: &mut AppState, right: bool) -> Vec<Effect> {
    match state.page.route {
        Route::Library => {
            let tabs = LibraryTab::ALL;
            let current = tabs.iter().position(|t| *t == state.page.library_tab).unwrap_or(0);
            let next = if right {
                (current + 1) % tabs.len()
            } else {
                current.checked_sub(1).unwrap_or(tabs.len() - 1)
            };
            let new_tab = tabs[next];
            state.library_tab = new_tab;

            let cache_key = match new_tab {
                LibraryTab::LikedSongs => "library:liked",
                LibraryTab::Albums => "library:albums",
                LibraryTab::Playlists => "library:playlists",
            };

            if let Some((cached_page, cached_at)) = state.page_cache.get(cache_key) {
                let is_fresh = cached_at.elapsed() < Duration::from_secs(60);
                let mut page = cached_page.clone();
                page.library_tab = new_tab;
                if is_fresh {
                    state.page = page;
                    return Vec::new();
                }
                state.generation = state.generation.saturating_add(1);
                page.generation = state.generation;
                state.page = page;
                vec![Effect::LoadPage {
                    route: Route::Library,
                    generation: state.generation,
                    offset: 0,
                    query: String::new(),
                    tab: Some(new_tab),
                }]
            } else {
                state.generation = state.generation.saturating_add(1);
                let mut page = PageState::loading(Route::Library, state.generation);
                page.library_tab = new_tab;
                state.page = page;
                vec![Effect::LoadPage {
                    route: Route::Library,
                    generation: state.generation,
                    offset: 0,
                    query: String::new(),
                    tab: Some(new_tab),
                }]
            }
        }
        Route::Search => {
            let current = SearchFilter::ALL
                .iter()
                .position(|filter| *filter == state.page.search_filter)
                .unwrap_or(0);
            let index = if right {
                (current + 1) % SearchFilter::ALL.len()
            } else {
                current.checked_sub(1).unwrap_or(SearchFilter::ALL.len() - 1)
            };
            state.page.search_filter = SearchFilter::ALL[index];
            state.page.cursor = crate::state::ListCursor::default();
            Vec::new()
        }
        Route::Settings => {
            let next_tab =
                if right { state.settings_tab.next() } else { state.settings_tab.previous() };
            switch_settings_tab(state, next_tab);
            Vec::new()
        }
        _ => Vec::new(),
    }
}

fn activate_device(state: &mut AppState, item: BrowseItem) -> Vec<Effect> {
    if item.id == LOCAL_DEVICE_PLACEHOLDER {
        if let Some(id) = state.local_device_id.clone() {
            state.local_start_requested = false;
            state.selected_device_id = Some(id.clone());
            state.device_selected_by_user = true;
            state.playback.device_id = Some(id.clone());
            state.playback.device_name = Some("This computer".into());
            state.overlay = None;
            return vec![Effect::TransferLocal(id)];
        }
        // Keep the currently reported Spotify device/playback state intact while the
        // local backend starts. Claiming "This computer" before Ready races with the
        // regular playback poller and makes the status/player UI flicker between devices.
        state.local_start_requested = true;
        state.overlay = None;
        state.notice = Some(Notice {
            kind: NoticeKind::Pending,
            text: format!("Starting {}…", state.local_playback_backend.label()),
        });
        vec![Effect::StartLocalPlayback]
    } else if item.restricted {
        state.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: "Spotify marks this device as restricted; commands cannot be transferred to it."
                .into(),
        });
        Vec::new()
    } else {
        item.uri.map_or_else(Vec::new, |id| {
            state.selected_device_id = Some(id.clone());
            state.device_selected_by_user = true;
            let clean_title = item.title.replace(" (active)", "").replace(" (restricted)", "");
            state.playback.device_name = Some(clean_title);
            state.playback.device_id = Some(id.clone());
            state.overlay = None;
            state.clear_error_notice();
            vec![Effect::Transfer(id)]
        })
    }
}

fn activate(state: &mut AppState) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::Menu)) {
        return AppState::NAVIGATION
            .get(state.sidebar.selected)
            .cloned()
            .map_or_else(Vec::new, |route| load_route(state, route));
    }
    if state.focus == FocusRegion::Sidebar {
        return AppState::NAVIGATION
            .get(state.sidebar.selected)
            .cloned()
            .map_or_else(Vec::new, |route| load_route(state, route));
    }
    let Some(item) = state.selected_item().cloned() else {
        return Vec::new();
    };
    match item.kind {
        EntityKind::Track => {
            if !item.available {
                state.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: "This track is unavailable".into(),
                });
                return Vec::new();
            }
            state.push_playback_history();
            state.apply_track_to_playback(&item);
            state.queue_now = Some(item.clone());
            if let Some(context) = item.context {
                vec![Effect::PlayContext {
                    uri: context.uri,
                    position: context.position,
                    device_id: state.target_device_id(),
                }]
            } else {
                item.uri.map_or_else(Vec::new, |uri| {
                    vec![Effect::PlayTrack { uri, device_id: state.target_device_id() }]
                })
            }
        }
        EntityKind::Album => item.uri.map_or_else(Vec::new, |uri| {
            load_route(state, Route::Album { uri, title: item.title })
        }),
        EntityKind::Playlist => item.uri.map_or_else(Vec::new, |uri| {
            load_route(state, Route::Playlist { uri, title: item.title })
        }),
        EntityKind::Artist => item.uri.map_or_else(Vec::new, |uri| {
            load_route(state, Route::Artist { uri, title: item.title })
        }),
        EntityKind::Device => activate_device(state, item),
        EntityKind::Action if !item.available => Vec::new(),
        EntityKind::Action if item.id.starts_with("recent-search:") => {
            let query = item.title.clone();
            state.search_query.clone_from(&query);
            state.recent_searches.retain(|q| !q.eq_ignore_ascii_case(&query));
            state.recent_searches.insert(0, query.clone());
            state.generation = state.generation.saturating_add(1);
            state.page.generation = state.generation;
            state.page.state = crate::state::LoadState::Loading;
            vec![
                Effect::LoadPage {
                    route: Route::Search,
                    generation: state.generation,
                    offset: 0,
                    query,
                    tab: None,
                },
                Effect::SaveSettings,
            ]
        }
        EntityKind::Action if item.id == "play-context" => item.uri.map_or_else(Vec::new, |uri| {
            vec![Effect::PlayTrack { uri, device_id: state.target_device_id() }]
        }),
        EntityKind::Action if item.id == "open-external" => {
            item.external_url.map_or_else(Vec::new, |url| vec![Effect::OpenExternal(url)])
        }
        EntityKind::Action => activate_setting(state, &item.id),
        EntityKind::Message => Vec::new(),
    }
}

fn toggle_saved(state: &mut AppState) -> Vec<Effect> {
    let Some(item) = state.selected_item().cloned() else {
        return Vec::new();
    };
    if item.kind != EntityKind::Track {
        return Vec::new();
    }
    let Some(uri) = item.uri.clone() else {
        return Vec::new();
    };
    let new_saved = match item.saved {
        Some(true) => false,
        Some(false) | None => true,
    };

    if new_saved {
        state.notice =
            Some(Notice { kind: NoticeKind::Success, text: "♥ Added to Liked Songs".into() });
    } else {
        state.notice =
            Some(Notice { kind: NoticeKind::Success, text: "♡ Removed from Liked Songs".into() });
    }

    state.page_cache.remove("library:liked");

    if state.page.route == Route::Library
        && state.page.library_tab == LibraryTab::LikedSongs
        && !new_saved
    {
        for section in &mut state.page.sections {
            section.items.retain(|i| i.uri.as_deref() != Some(&uri));
        }
        let len = state.page.flattened().len();
        if len == 0 {
            state.page.cursor.selected = 0;
            state.page.cursor.offset = 0;
        } else if state.page.cursor.selected >= len {
            state.page.cursor.selected = len - 1;
        }
    } else {
        for section in &mut state.page.sections {
            for track in &mut section.items {
                if track.uri.as_deref() == Some(&uri) {
                    track.saved = Some(new_saved);
                }
            }
        }
    }

    for track in &mut state.queue_upcoming {
        if track.uri.as_deref() == Some(&uri) {
            track.saved = Some(new_saved);
        }
    }
    if let Some(now) = &mut state.queue_now
        && now.uri.as_deref() == Some(&uri)
    {
        now.saved = Some(new_saved);
    }

    vec![Effect::SetSaved { uri, saved: new_saved }]
}

fn playlist_id_from_uri(uri: &str) -> String {
    if let Some(id) = uri.strip_prefix("spotify:playlist:") {
        id.to_string()
    } else {
        uri.to_string()
    }
}

fn activate_playlist_picker(
    state: &mut AppState,
    selected: usize,
    playlists: &[BrowseItem],
    pending_uri: &str,
) -> Vec<Effect> {
    let Some(playlist) = playlists.get(selected) else {
        return Vec::new();
    };
    let playlist_id =
        playlist.uri.as_deref().map_or_else(|| playlist.id.clone(), playlist_id_from_uri);
    let track_uri = pending_uri.to_string();
    let title = playlist.title.clone();
    state.overlay = None;
    state.notice =
        Some(Notice { kind: NoticeKind::Pending, text: format!("Adding track to {title}…") });
    vec![Effect::AddToPlaylist { playlist_id, track_uri }]
}

#[allow(clippy::too_many_lines)]
fn activate_action_menu(
    state: &mut AppState,
    selected: usize,
    actions: &[ContextAction],
) -> Vec<Effect> {
    let Some(action) = actions.get(selected).copied() else {
        state.overlay = None;
        return Vec::new();
    };
    let item = state.selected_item().cloned();
    state.overlay = None;
    let Some(item) = item else {
        return Vec::new();
    };

    match action {
        ContextAction::Play => activate(state),
        ContextAction::PlayNext => {
            if let Some(uri) = item.uri.clone() {
                state.queue_upcoming.insert(0, item.clone());
                state.notice = Some(Notice {
                    kind: NoticeKind::Pending,
                    text: format!("Playing {} next", item.title),
                });
                vec![Effect::Enqueue(uri)]
            } else {
                Vec::new()
            }
        }
        ContextAction::AddToQueue => enqueue_selected(state),
        ContextAction::Like => {
            if item.saved == Some(true) {
                Vec::new()
            } else {
                toggle_saved(state)
            }
        }
        ContextAction::Unlike => {
            if item.saved == Some(false) {
                Vec::new()
            } else {
                toggle_saved(state)
            }
        }
        ContextAction::AddToPlaylist => {
            if let Some(uri) = item.uri.clone() {
                state.overlay = Some(Overlay::PlaylistPicker {
                    selected: 0,
                    playlists: Vec::new(),
                    pending_uri: uri.clone(),
                });
                vec![Effect::LoadPlaylistsForPicker { pending_uri: uri }]
            } else {
                Vec::new()
            }
        }
        ContextAction::RemoveFromPlaylist => {
            let playlist_uri = state.page.uri.clone().unwrap_or_else(|| match &state.page.route {
                Route::Playlist { uri, .. } => uri.clone(),
                _ => String::new(),
            });
            if let Some(track_uri) = item.uri.clone() {
                for section in &mut state.page.sections {
                    section.items.retain(|i| i.uri.as_deref() != Some(&track_uri));
                }
                let len = state.page.flattened().len();
                if len == 0 {
                    state.page.cursor.selected = 0;
                    state.page.cursor.offset = 0;
                } else if state.page.cursor.selected >= len {
                    state.page.cursor.selected = len - 1;
                }
                state.notice = Some(Notice {
                    kind: NoticeKind::Pending,
                    text: format!("Removing {} from playlist…", item.title),
                });
                vec![Effect::RemoveFromPlaylist { playlist_uri, track_uri }]
            } else {
                Vec::new()
            }
        }
        ContextAction::SaveAlbum => {
            if let Some(uri) = item.uri.clone() {
                state.notice = Some(Notice {
                    kind: NoticeKind::Success,
                    text: "Saved album to library".into(),
                });
                state.page_cache.remove("library:albums");
                for section in &mut state.page.sections {
                    for album in &mut section.items {
                        if album.uri.as_deref() == Some(&uri) {
                            album.saved = Some(true);
                        }
                    }
                }
                vec![Effect::SetSaved { uri, saved: true }]
            } else {
                Vec::new()
            }
        }
        ContextAction::RemoveSavedAlbum => {
            if let Some(uri) = item.uri.clone() {
                state.notice = Some(Notice {
                    kind: NoticeKind::Success,
                    text: "Removed album from library".into(),
                });
                state.page_cache.remove("library:albums");
                if state.page.route == Route::Library
                    && state.page.library_tab == LibraryTab::Albums
                {
                    for section in &mut state.page.sections {
                        section.items.retain(|i| i.uri.as_deref() != Some(&uri));
                    }
                    let len = state.page.flattened().len();
                    if len == 0 {
                        state.page.cursor.selected = 0;
                        state.page.cursor.offset = 0;
                    } else if state.page.cursor.selected >= len {
                        state.page.cursor.selected = len - 1;
                    }
                } else {
                    for section in &mut state.page.sections {
                        for album in &mut section.items {
                            if album.uri.as_deref() == Some(&uri) {
                                album.saved = Some(false);
                            }
                        }
                    }
                }
                vec![Effect::SetSaved { uri, saved: false }]
            } else {
                Vec::new()
            }
        }
        ContextAction::FollowArtist => {
            if let Some(uri) = item.uri.clone() {
                state.notice =
                    Some(Notice { kind: NoticeKind::Success, text: "Following artist".into() });
                for section in &mut state.page.sections {
                    for artist in &mut section.items {
                        if artist.uri.as_deref() == Some(&uri) {
                            artist.saved = Some(true);
                        }
                    }
                }
                vec![Effect::SetSaved { uri, saved: true }]
            } else {
                Vec::new()
            }
        }
        ContextAction::UnfollowArtist => {
            if let Some(uri) = item.uri.clone() {
                state.notice =
                    Some(Notice { kind: NoticeKind::Success, text: "Unfollowed artist".into() });
                for section in &mut state.page.sections {
                    for artist in &mut section.items {
                        if artist.uri.as_deref() == Some(&uri) {
                            artist.saved = Some(false);
                        }
                    }
                }
                vec![Effect::SetSaved { uri, saved: false }]
            } else {
                Vec::new()
            }
        }
        ContextAction::PlayContext => item.uri.map_or_else(Vec::new, |uri| {
            vec![Effect::PlayTrack { uri, device_id: state.target_device_id() }]
        }),
        ContextAction::ShuffleContext => item.uri.map_or_else(Vec::new, |uri| {
            state.playback.shuffle = true;
            vec![
                Effect::PlayTrack { uri, device_id: state.target_device_id() },
                Effect::Shuffle(true),
            ]
        }),
        ContextAction::GoToAlbum => item
            .album
            .map_or_else(Vec::new, |(title, uri)| load_route(state, Route::Album { uri, title })),
        ContextAction::GoToArtist => item
            .artists
            .first()
            .and_then(|(title, uri)| uri.as_ref().map(|uri| (title.clone(), uri.clone())))
            .map_or_else(Vec::new, |(title, uri)| load_route(state, Route::Artist { uri, title })),
        ContextAction::RenamePlaylist => {
            let playlist_id =
                item.uri.as_deref().map_or_else(|| item.id.clone(), playlist_id_from_uri);
            state.overlay = Some(Overlay::RenamePlaylist { playlist_id, name: item.title.clone() });
            state.text_entry = true;
            Vec::new()
        }
        ContextAction::DeletePlaylist => {
            if let Some(uri) = item.uri.clone() {
                state.notice = Some(Notice {
                    kind: NoticeKind::Success,
                    text: "Removed playlist from library".into(),
                });
                state.page_cache.remove("library:playlists");
                if state.page.route == Route::Library
                    && state.page.library_tab == LibraryTab::Playlists
                {
                    for section in &mut state.page.sections {
                        section.items.retain(|i| i.uri.as_deref() != Some(&uri));
                    }
                    let len = state.page.flattened().len();
                    if len == 0 {
                        state.page.cursor.selected = 0;
                        state.page.cursor.offset = 0;
                    } else if state.page.cursor.selected >= len {
                        state.page.cursor.selected = len - 1;
                    }
                }
                vec![Effect::SetSaved { uri, saved: false }]
            } else {
                Vec::new()
            }
        }
        ContextAction::OpenInSpotify => {
            let url = item.external_url.clone().or_else(|| {
                item.uri.as_ref().and_then(|uri| {
                    uri.parse::<lspotify_core::ids::SpotifyUri>()
                        .ok()
                        .map(|parsed| parsed.web_url())
                })
            });
            url.map_or_else(Vec::new, |url| vec![Effect::OpenExternal(url)])
        }
        ContextAction::EditShortcut => {
            if let Some(action_id) = item.id.strip_prefix("shortcut:")
                && let Some(action) = ShortcutAction::from_id(action_id)
            {
                state.overlay = Some(Overlay::ShortcutCapture { action, conflict: None });
            }
            Vec::new()
        }
        ContextAction::RemoveShortcutBinding => {
            if let Some(action_id) = item.id.strip_prefix("shortcut:")
                && let Some(action) = ShortcutAction::from_id(action_id)
            {
                state.shortcuts.remove_binding(action);
                refresh_settings_page(state);
                return vec![Effect::SaveSettings];
            }
            Vec::new()
        }
        ContextAction::ResetShortcutToDefault => {
            if let Some(action_id) = item.id.strip_prefix("shortcut:")
                && let Some(action) = ShortcutAction::from_id(action_id)
            {
                state.shortcuts.restore_default(action);
                refresh_settings_page(state);
                return vec![Effect::SaveSettings];
            }
            Vec::new()
        }
        ContextAction::ResetAllShortcutsToDefault => {
            state.shortcuts.restore_all_defaults();
            refresh_settings_page(state);
            vec![Effect::SaveSettings]
        }
    }
}

#[allow(clippy::too_many_lines)]
fn activate_setting(state: &mut AppState, id: &str) -> Vec<Effect> {
    if let Some(action_id) = id.strip_prefix("shortcut:")
        && let Some(action) = ShortcutAction::from_id(action_id)
    {
        state.overlay = Some(Overlay::ShortcutCapture { action, conflict: None });
        return Vec::new();
    }
    match id {
        "artwork" => {
            state.artwork = match state.artwork {
                lspotify_core::CliArtworkPreference::Auto => {
                    lspotify_core::CliArtworkPreference::Blocks
                }
                lspotify_core::CliArtworkPreference::Blocks => {
                    lspotify_core::CliArtworkPreference::Off
                }
                lspotify_core::CliArtworkPreference::Off => {
                    lspotify_core::CliArtworkPreference::Auto
                }
            }
        }
        "artwork-under-overlays" => state.artwork_under_overlays = !state.artwork_under_overlays,
        "mouse" => state.mouse = !state.mouse,
        "wide-queue" => state.wide_queue = !state.wide_queue,
        "side-player-height" => {
            const VALUES: &[u16] = &[28, 30, 32, 0, 24, 26];
            let next_idx = VALUES
                .iter()
                .position(|&v| v == state.side_player_max_height)
                .map_or(0, |idx| (idx + 1) % VALUES.len());
            state.side_player_max_height = VALUES[next_idx];
        }
        "side-player-width" => {
            const VALUES: &[u16] = &[100, 110, 120, 80, 90];
            let next_idx = VALUES
                .iter()
                .position(|&v| v == state.side_player_min_width)
                .map_or(0, |idx| (idx + 1) % VALUES.len());
            state.side_player_min_width = VALUES[next_idx];
        }
        "stacked-queue-height" => {
            const VALUES: &[u16] = &[38, 42, 46, 999, 34];
            let next_idx = VALUES
                .iter()
                .position(|&v| v == state.stacked_queue_min_height)
                .map_or(0, |idx| (idx + 1) % VALUES.len());
            state.stacked_queue_min_height = VALUES[next_idx];
        }
        "wide-breakpoint" => {
            const VALUES: &[u16] = &[140, 150, 160, 120, 130];
            let next_idx = VALUES
                .iter()
                .position(|&v| v == state.wide_breakpoint_width)
                .map_or(0, |idx| (idx + 1) % VALUES.len());
            state.wide_breakpoint_width = VALUES[next_idx];
        }
        "playback-backend" => {
            if !cfg!(target_os = "windows") {
                return Vec::new();
            }
            state.local_playback_backend = match state.local_playback_backend {
                LocalPlaybackBackendPreference::SpotifyWeb => {
                    LocalPlaybackBackendPreference::Librespot
                }
                LocalPlaybackBackendPreference::Librespot => {
                    LocalPlaybackBackendPreference::SpotifyWeb
                }
            };
            refresh_settings_page(state);
            return vec![
                Effect::ChangeLocalPlaybackBackend(state.local_playback_backend),
                Effect::SaveSettings,
            ];
        }
        _ => return Vec::new(),
    }
    state.update_layout(state.terminal_size.0, state.terminal_size.1);
    if let Some(item) = state
        .page
        .sections
        .iter_mut()
        .flat_map(|section| &mut section.items)
        .find(|item| item.id == id)
    {
        item.title = match id {
            "artwork" => format!("Artwork: {:?}", state.artwork),
            "artwork-under-overlays" => format!(
                "Artwork behind menus: {}",
                if state.artwork_under_overlays { "enabled" } else { "disabled" }
            ),
            "mouse" => format!("Mouse input: {}", if state.mouse { "enabled" } else { "disabled" }),
            "wide-queue" => format!(
                "Wide-screen queue: {}",
                if state.wide_queue { "enabled" } else { "disabled" }
            ),
            "side-player-height" => {
                if state.side_player_max_height == 0 {
                    "Side player max height: Disabled".into()
                } else {
                    format!("Side player max height: {} rows", state.side_player_max_height)
                }
            }
            "side-player-width" => {
                format!("Side player min width: {} cols", state.side_player_min_width)
            }
            "stacked-queue-height" => {
                if state.stacked_queue_min_height > 500 {
                    "Stacked queue min height: Disabled".into()
                } else {
                    format!("Stacked queue min height: {} rows", state.stacked_queue_min_height)
                }
            }
            "wide-breakpoint" => {
                format!("Wide layout min width: {} cols", state.wide_breakpoint_width)
            }
            "playback-backend" => {
                format!("Local playback engine: {}", state.local_playback_backend.label())
            }
            _ => item.title.clone(),
        };
    }
    if state.page.route == Route::Settings {
        state.page_cache.insert(Route::Settings.key(), (state.page.clone(), Instant::now()));
    }
    vec![Effect::SaveSettings]
}

fn setting_item(id: &str, title: &str) -> BrowseItem {
    let mut item = BrowseItem::message(id, title);
    item.kind = EntityKind::Action;
    item.available = true;
    item
}

#[must_use]
pub fn general_settings_sections(state: &AppState) -> Vec<Section> {
    vec![Section {
        title: "Terminal".into(),
        items: vec![
            setting_item("artwork", &format!("Artwork: {:?}", state.artwork)),
            setting_item(
                "artwork-under-overlays",
                &format!(
                    "Artwork behind menus: {}",
                    if state.artwork_under_overlays { "enabled" } else { "disabled" }
                ),
            ),
            setting_item(
                "mouse",
                &format!("Mouse input: {}", if state.mouse { "enabled" } else { "disabled" }),
            ),
            setting_item(
                "wide-queue",
                &format!(
                    "Wide-screen queue: {}",
                    if state.wide_queue { "enabled" } else { "disabled" }
                ),
            ),
            setting_item(
                "side-player-height",
                &if state.side_player_max_height == 0 {
                    "Side player max height: Disabled".into()
                } else {
                    format!("Side player max height: {} rows", state.side_player_max_height)
                },
            ),
            setting_item(
                "side-player-width",
                &format!("Side player min width: {} cols", state.side_player_min_width),
            ),
            setting_item(
                "stacked-queue-height",
                &if state.stacked_queue_min_height > 500 {
                    "Stacked queue min height: Disabled".into()
                } else {
                    format!("Stacked queue min height: {} rows", state.stacked_queue_min_height)
                },
            ),
            setting_item(
                "wide-breakpoint",
                &format!("Wide layout min width: {} cols", state.wide_breakpoint_width),
            ),
        ],
    }]
}

#[must_use]
pub fn playback_settings_sections(state: &AppState) -> Vec<Section> {
    let mut backend = BrowseItem::message(
        "playback-backend",
        format!("Local playback engine: {}", state.local_playback_backend.label()),
    );
    backend.kind = EntityKind::Action;
    backend.available = cfg!(target_os = "windows");
    backend.subtitle = if cfg!(target_os = "windows") {
        match state.local_playback_backend {
            LocalPlaybackBackendPreference::SpotifyWeb => {
                "Official Spotify Web Playback SDK · higher memory use".into()
            }
            LocalPlaybackBackendPreference::Librespot => {
                "Lightweight native playback · unofficial Spotify implementation".into()
            }
        }
    } else {
        "Librespot is the only local playback engine available on this platform.".into()
    };
    backend.metadata = match state.local_playback_backend {
        LocalPlaybackBackendPreference::Librespot => {
            "Librespot registers at startup; choose This computer to transfer playback.".into()
        }
        LocalPlaybackBackendPreference::SpotifyWeb => {
            "Web Playback starts when selected, or immediately after switching to this backend."
                .into()
        }
    };
    vec![Section { title: "Local playback".into(), items: vec![backend] }]
}

#[must_use]
pub fn shortcut_settings_sections(state: &AppState) -> Vec<Section> {
    let mut items = Vec::new();
    for &action in &ShortcutAction::ALL {
        let mut item =
            BrowseItem::message(format!("shortcut:{}", action.id()), action.display_name());
        item.kind = EntityKind::Action;
        item.subtitle = state.shortcuts.list_label(action, ", ");
        item.metadata = format!("Scope: {}", action.scope());
        item.available = true;
        items.push(item);
    }
    vec![Section { title: "Keybindings".into(), items }]
}

pub fn refresh_settings_page(state: &mut AppState) {
    if state.page.route != Route::Settings {
        return;
    }
    state.page.title = "Settings".into();
    state.page.subtitle = match state.settings_tab {
        SettingsTab::General => "CLI appearance and interaction".into(),
        SettingsTab::Playback => "Local playback engine".into(),
        SettingsTab::Shortcuts => "Configure keyboard shortcuts".into(),
    };
    state.page.sections = match state.settings_tab {
        SettingsTab::General => general_settings_sections(state),
        SettingsTab::Playback => playback_settings_sections(state),
        SettingsTab::Shortcuts => shortcut_settings_sections(state),
    };
    state.page.state = crate::state::LoadState::Ready;
    let len = state.page.flattened().len();
    if state.page.cursor.selected >= len {
        state.page.cursor.selected = len.saturating_sub(1);
    }
    state.page_cache.insert(Route::Settings.key(), (state.page.clone(), Instant::now()));
}

pub fn switch_settings_tab(state: &mut AppState, tab: SettingsTab) {
    state.settings_tab = tab;
    state.page.settings_tab = tab;
    state.page.cursor = crate::state::ListCursor::default();
    refresh_settings_page(state);
}

fn enqueue_selected(state: &mut AppState) -> Vec<Effect> {
    let Some(item) = state.selected_item() else {
        return Vec::new();
    };
    if item.kind != EntityKind::Track || !item.available {
        return Vec::new();
    }
    let uri = item.uri.clone();
    if let Some(uri) = uri {
        state.notice = Some(Notice {
            kind: NoticeKind::Pending,
            text: format!("Adding {} to queue…", item.title),
        });
        vec![Effect::Enqueue(uri)]
    } else {
        Vec::new()
    }
}

fn insert_text(state: &mut AppState, ch: char) {
    if let Some(Overlay::Help { query, .. }) = &mut state.overlay {
        query.push(ch);
    } else if let Some(Overlay::RenamePlaylist { name, .. }) = &mut state.overlay {
        name.push(ch);
    } else if state.page.route == Route::Search {
        state.search_query.push(ch);
    } else {
        state.page.filter.push(ch);
    }
}

fn backspace(state: &mut AppState) {
    if let Some(Overlay::Help { query, .. }) = &mut state.overlay {
        query.pop();
    } else if let Some(Overlay::RenamePlaylist { name, .. }) = &mut state.overlay {
        name.pop();
    } else if state.page.route == Route::Search {
        state.search_query.pop();
    } else {
        state.page.filter.pop();
    }
}

fn submit_text(state: &mut AppState) -> Vec<Effect> {
    if matches!(state.overlay, Some(Overlay::RenamePlaylist { .. }))
        && let Some(Overlay::RenamePlaylist { playlist_id, name }) = state.overlay.take()
    {
        state.text_entry = false;
        let trimmed = name.trim().to_string();
        if !trimmed.is_empty() {
            state.notice = Some(Notice {
                kind: NoticeKind::Pending,
                text: format!("Renaming playlist to \"{trimmed}\"…"),
            });
            return vec![Effect::RenamePlaylist { playlist_id, new_name: trimmed }];
        }
        return Vec::new();
    }
    if let Some(Overlay::Help { editing, .. }) = &mut state.overlay {
        *editing = false;
        state.text_entry = false;
        return Vec::new();
    }
    if state.page.route == Route::Search {
        let trimmed = state.search_query.trim().to_string();
        if trimmed.is_empty() {
            state.populate_recent_searches();
            return Vec::new();
        }
        state.text_entry = false;
        state.recent_searches.retain(|q| !q.eq_ignore_ascii_case(&trimmed));
        state.recent_searches.insert(0, trimmed);
        if state.recent_searches.len() > 10 {
            state.recent_searches.truncate(10);
        }
        state.generation = state.generation.saturating_add(1);
        state.page.generation = state.generation;
        state.page.state = crate::state::LoadState::Loading;
        vec![
            Effect::LoadPage {
                route: Route::Search,
                generation: state.generation,
                offset: 0,
                query: state.search_query.clone(),
                tab: None,
            },
            Effect::SaveSettings,
        ]
    } else {
        state.text_entry = false;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_ready_local_device_transfers_once_without_waiting_for_ready() {
        let mut state =
            AppState { local_device_id: Some("local-id".into()), ..AppState::default() };
        let mut device = BrowseItem::message(LOCAL_DEVICE_PLACEHOLDER, "This computer");
        device.kind = EntityKind::Device;
        state.page.sections = vec![Section { title: "Devices".into(), items: vec![device] }];
        assert_eq!(activate(&mut state), vec![Effect::TransferLocal("local-id".into())]);
        assert!(!state.local_start_requested);
        assert_eq!(state.selected_device_id.as_deref(), Some("local-id"));
    }

    #[test]
    fn shuffle_and_repeat_mouse_targets_dispatch_only_inside_their_rectangles() {
        let now = Instant::now();
        let mut state = AppState::default();
        let mut hits = HitMap::default();
        hits.add(Rect { x: 4, y: 2, width: 12, height: 1 }, HitTarget::PlayerShuffle);
        hits.add(Rect { x: 17, y: 2, width: 12, height: 1 }, HitTarget::PlayerRepeat);
        let click = |column| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 2,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            dispatch_mouse(&mut state, &mut hits, click(5), now),
            vec![Effect::Shuffle(true)]
        );
        assert_eq!(
            dispatch_mouse(&mut state, &mut hits, click(18), now + Duration::from_secs(1)),
            vec![Effect::Repeat(1)]
        );
        assert!(
            dispatch_mouse(&mut state, &mut hits, click(16), now + Duration::from_secs(2))
                .is_empty()
        );
    }

    use crate::{BrowseItem, ContextPosition, LoadState, PageState, Section};
    use crossterm::event::{KeyEvent, MouseEvent};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn focus_isolated_and_cycles_visible_regions() {
        let mut state = AppState::default();
        state.update_layout(160, 45);
        dispatch_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, FocusRegion::Queue);
        dispatch_key(&mut state, key(KeyCode::Tab));
        assert_eq!(state.focus, FocusRegion::Player);
    }

    #[test]
    fn text_entry_consumes_playback_shortcuts() {
        let mut state = AppState::default();
        dispatch_key(&mut state, key(KeyCode::Char('/')));
        let effects = dispatch_key(&mut state, key(KeyCode::Char(' ')));
        assert!(effects.is_empty());
        assert_eq!(state.search_query, " ");
    }

    #[test]
    fn quit_shortcuts_are_global_without_claiming_lowercase_queue() {
        let quit_keys = [
            key(KeyCode::Char('x')),
            key(KeyCode::Char('Q')),
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
        ];

        let contexts = [
            (false, None),
            (true, None),
            (false, Some(Overlay::Inspector)),
            (false, Some(Overlay::Help { query: String::new(), editing: false })),
            (false, Some(Overlay::Help { query: String::new(), editing: true })),
            (
                false,
                Some(Overlay::RenamePlaylist { playlist_id: "p1".into(), name: "test".into() }),
            ),
            (false, Some(Overlay::Actions { selected: 0, actions: vec![] })),
            (
                false,
                Some(Overlay::PlaylistPicker {
                    selected: 0,
                    playlists: vec![],
                    pending_uri: "track".into(),
                }),
            ),
            (false, Some(Overlay::DevicePicker)),
            (false, Some(Overlay::Menu)),
            (true, Some(Overlay::Help { query: "query".into(), editing: true })),
            (
                false,
                Some(Overlay::ShortcutCapture {
                    action: ShortcutAction::TogglePlayback,
                    conflict: None,
                }),
            ),
        ];

        for key_event in quit_keys {
            for (text_entry, overlay) in &contexts {
                let mut state = AppState {
                    overlay: overlay.clone(),
                    text_entry: *text_entry,
                    ..AppState::default()
                };
                dispatch_key(&mut state, key_event);
                let typing = *text_entry
                    || matches!(
                        overlay,
                        Some(Overlay::Help { editing: true, .. } | Overlay::RenamePlaylist { .. })
                    );
                let printable = matches!(key_event.code, KeyCode::Char(_))
                    && !key_event.modifiers.contains(KeyModifiers::CONTROL);
                assert_eq!(
                    state.quit,
                    !(typing && printable),
                    "Key {key_event:?} quit mismatch with text_entry={text_entry}, overlay={overlay:?}"
                );
            }
        }

        let mut state = AppState::default();
        dispatch_key(&mut state, key(KeyCode::Char('q')));
        assert_eq!(state.page.route, Route::Queue);
        assert!(!state.quit);
    }

    #[test]
    fn history_restores_selection_scroll_and_filter() {
        let mut state = AppState::default();
        state.page.cursor.selected = 8;
        state.page.cursor.offset = 4;
        state.page.filter = "mix".into();
        reduce(&mut state, Action::Navigate(Route::Library));
        reduce(&mut state, Action::Cancel);
        assert_eq!(state.page.cursor.selected, 8);
        assert_eq!(state.page.cursor.offset, 4);
        assert_eq!(state.page.filter, "mix");
    }

    #[test]
    fn progress_seek_uses_the_visible_track_and_clamps() {
        let track = Rect { x: 20, y: 3, width: 5, height: 1 };
        assert_eq!(seek_position_at(track, 20, 200), Some(0));
        assert_eq!(seek_position_at(track, 21, 200), Some(50));
        assert_eq!(seek_position_at(track, 22, 200), Some(100));
        assert_eq!(seek_position_at(track, 23, 200), Some(150));
        assert_eq!(seek_position_at(track, 24, 200), Some(200));
        assert_eq!(seek_position_at(track, 2, 200), Some(0));
        assert_eq!(seek_position_at(track, 40, 200), Some(200));
        assert_eq!(seek_position_at(track, 20, 0), None);
    }
    #[test]
    fn shift_arrows_seek_while_plain_arrows_are_local() {
        let mut state = AppState::default();
        let seek = dispatch_key(&mut state, KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT));
        assert_eq!(seek, vec![Effect::Seek(5_000)]);
        assert!(dispatch_key(&mut state, key(KeyCode::Right)).is_empty());
    }

    #[test]
    fn escape_closes_overlay_before_restoring_history() {
        let mut state = AppState::default();
        state.history.push(PageState::loading(Route::Library, 0));
        state.overlay = Some(Overlay::Inspector);
        dispatch_key(&mut state, key(KeyCode::Esc));
        assert!(state.overlay.is_none());
        assert_eq!(state.page.route, Route::Home);
    }

    #[test]
    fn compact_menu_activation_uses_sidebar_selection() {
        let mut state = AppState::default();
        state.update_layout(80, 24);
        state.overlay = Some(Overlay::Menu);
        state.sidebar.selected = 2;
        let effects = dispatch_key(&mut state, key(KeyCode::Enter));
        assert!(matches!(state.page.route, Route::Library));
        assert!(matches!(effects.as_slice(), [Effect::LoadPage { route: Route::Library, .. }]));
    }

    #[test]
    fn sidebar_mouse_click_navigates_immediately() {
        let mut state = AppState::default();
        state.update_layout(120, 35);
        let mut hits = HitMap::default();
        hits.add(Rect { x: 0, y: 0, width: 18, height: 1 }, HitTarget::Sidebar(2));
        let effects = dispatch_mouse(
            &mut state,
            &mut hits,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 2,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
            Instant::now(),
        );
        assert_eq!(state.page.route, Route::Library);
        assert!(matches!(effects.as_slice(), [Effect::LoadPage { route: Route::Library, .. }]));
    }

    #[test]
    fn browse_play_enqueue_queue_and_back_preserves_playlist_position() {
        let mut library = page_with(
            Route::Library,
            vec![item(EntityKind::Playlist, "Mix", "spotify:playlist:mix", None)],
        );
        library.library_tab = LibraryTab::Playlists;
        let mut state = AppState { page: library, ..AppState::default() };
        let open = reduce(&mut state, Action::Activate);
        assert!(matches!(
            open.as_slice(),
            [Effect::LoadPage { route: Route::Playlist { .. }, .. }]
        ));

        state.page = page_with(
            Route::Playlist { uri: "spotify:playlist:mix".into(), title: "Mix".into() },
            vec![
                item(EntityKind::Track, "First", "spotify:track:one", Some(0)),
                item(EntityKind::Track, "Middle", "spotify:track:same", Some(1)),
                item(EntityKind::Track, "Duplicate", "spotify:track:same", Some(2)),
            ],
        );
        state.page.cursor.selected = 1;
        state.page.cursor.offset = 1;
        let play = reduce(&mut state, Action::Activate);
        assert!(matches!(play.as_slice(), [Effect::PlayContext { position: 1, .. }]));
        let snapshot = state.page.clone();

        reduce(
            &mut state,
            Action::Navigate(Route::Artist {
                uri: "spotify:artist:a".into(),
                title: "Artist".into(),
            }),
        );
        state.page = page_with(
            Route::Artist { uri: "spotify:artist:a".into(), title: "Artist".into() },
            vec![item(EntityKind::Album, "Album", "spotify:album:a", None)],
        );
        reduce(&mut state, Action::Activate);
        state.page = page_with(
            Route::Album { uri: "spotify:album:a".into(), title: "Album".into() },
            vec![item(EntityKind::Track, "Song", "spotify:track:song", Some(0))],
        );
        assert!(
            matches!(reduce(&mut state, Action::Enqueue).as_slice(), [Effect::Enqueue(uri)] if uri == "spotify:track:song")
        );
        reduce(&mut state, Action::Navigate(Route::Queue));
        state.history.push(snapshot.clone());
        state.back();
        assert_eq!(state.page.route, snapshot.route);
        assert_eq!(state.page.cursor, snapshot.cursor);
    }

    fn page_with(route: Route, items: Vec<BrowseItem>) -> PageState {
        let mut page = PageState::loading(route, 0);
        page.state = LoadState::Ready;
        page.sections = vec![Section { title: "Items".into(), items }];
        page
    }

    fn item(kind: EntityKind, title: &str, uri: &str, position: Option<usize>) -> BrowseItem {
        BrowseItem {
            id: format!("{title}-{position:?}"),
            kind,
            title: title.into(),
            subtitle: "Artist".into(),
            metadata: String::new(),
            uri: Some(uri.into()),
            external_url: None,
            artwork_url: None,
            artists: vec![("Artist".into(), Some("spotify:artist:a".into()))],
            album: Some(("Album".into(), "spotify:album:a".into())),
            duration_ms: Some(180_000),
            available: true,
            saved: None,
            context: position
                .map(|position| ContextPosition { uri: "spotify:playlist:mix".into(), position }),
            restricted: false,
        }
    }

    #[test]
    fn next_action_advances_queue_and_updates_playback() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:current".into());
        state.playback.title = "Current".into();
        state.queue_upcoming = vec![
            item(EntityKind::Track, "Upcoming 1", "spotify:track:up1", None),
            item(EntityKind::Track, "Upcoming 2", "spotify:track:up2", None),
        ];

        let effects = reduce(&mut state, Action::Next);
        assert_eq!(effects, vec![Effect::Next]);
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:up1"));
        assert_eq!(state.playback.title, "Upcoming 1");
        assert_eq!(state.queue_upcoming.len(), 1);
        assert_eq!(state.queue_upcoming[0].title, "Upcoming 2");
        assert_eq!(state.playback_history.len(), 1);
        assert_eq!(state.playback_history[0].title, "Current");
    }

    #[test]
    fn previous_action_rewinds_if_over_three_seconds() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:current".into());
        state.playback.title = "Current".into();
        state.playback.duration_ms = 200_000;
        state.playback.progress_ms = 10_000;
        state.playback.playing = false;

        let effects = reduce(&mut state, Action::Previous);
        assert_eq!(effects, vec![Effect::Previous]);
        assert_eq!(state.playback.progress_ms, 0);
        assert_eq!(state.playback.title, "Current");
    }

    #[test]
    fn previous_action_restores_from_history_if_under_three_seconds() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:song1".into());
        state.playback.title = "Song 1".into();
        state.push_playback_history();

        state.playback.track_uri = Some("spotify:track:song2".into());
        state.playback.title = "Song 2".into();
        state.playback.progress_ms = 1_000;
        state.playback.playing = false;

        let effects = reduce(&mut state, Action::Previous);
        assert_eq!(effects, vec![Effect::Previous]);
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:song1"));
        assert_eq!(state.playback.title, "Song 1");
        assert_eq!(state.playback.progress_ms, 0);
        assert!(state.playback_history.is_empty());
        // Song 2 was prepended back to queue_upcoming so subsequent Next preserves it
        assert_eq!(state.queue_upcoming.len(), 1);
        assert_eq!(state.queue_upcoming[0].title, "Song 2");
        assert_eq!(state.queue_now.as_ref().map(|n| n.title.as_str()), Some("Song 1"));
    }

    #[test]
    fn next_action_when_queue_empty_does_not_push_to_history() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:current".into());
        state.playback.title = "Current".into();
        state.queue_upcoming.clear();

        let effects = reduce(&mut state, Action::Next);
        assert_eq!(effects, vec![Effect::Next]);
        assert_eq!(state.playback.title, "Current");
        assert!(state.playback_history.is_empty());
    }

    #[test]
    fn activate_track_optimistically_updates_playback() {
        let mut state = AppState::default();
        state.playback.track_uri = Some("spotify:track:initial".into());
        state.playback.title = "Initial".into();
        state.page = page_with(
            Route::Home,
            vec![item(EntityKind::Track, "Activated Track", "spotify:track:activated", None)],
        );
        state.page.cursor.selected = 0;

        let effects = reduce(&mut state, Action::Activate);
        assert!(
            matches!(effects.as_slice(), [Effect::PlayTrack { uri, .. }] if uri == "spotify:track:activated")
        );
        assert_eq!(state.playback.track_uri.as_deref(), Some("spotify:track:activated"));
        assert_eq!(state.playback.title, "Activated Track");
        assert_eq!(state.playback_history.len(), 1);
        assert_eq!(state.playback_history[0].title, "Initial");
    }

    #[test]
    fn navigation_shortcuts_one_to_four_switch_tabs() {
        let mut state = AppState::default();
        let _ = dispatch_key(&mut state, key(KeyCode::Char('2')));
        assert_eq!(state.page.route, Route::Search);
        assert!(state.text_entry);

        // Exiting text entry allows numeric shortcut navigation
        let _ = dispatch_key(&mut state, key(KeyCode::Down));
        assert!(!state.text_entry);

        let _ = dispatch_key(&mut state, key(KeyCode::Char('3')));
        assert_eq!(state.page.route, Route::Library);

        let _ = dispatch_key(&mut state, key(KeyCode::Char('4')));
        assert_eq!(state.page.route, Route::Settings);

        let _ = dispatch_key(&mut state, key(KeyCode::Char('1')));
        assert_eq!(state.page.route, Route::Home);
    }

    #[test]
    fn navigating_to_search_auto_selects_input_and_enter_does_not_activate_track() {
        let mut state = AppState {
            page: page_with(
                Route::Home,
                vec![item(EntityKind::Track, "Initial Track", "spotify:track:initial", None)],
            ),
            ..AppState::default()
        };

        // Press '2' to go to Search
        let _ = dispatch_key(&mut state, key(KeyCode::Char('2')));
        assert_eq!(state.page.route, Route::Search);
        assert!(state.text_entry, "Search input should be auto-selected");

        // Pressing Enter immediately on empty search query does NOT play music
        let effects = dispatch_key(&mut state, key(KeyCode::Enter));
        assert!(effects.is_empty(), "Enter on empty search should not emit play or load effects");
        assert_eq!(state.playback.track_uri, None, "Track must not start playing");
        assert!(state.text_entry, "Should remain in text entry if empty query submitted");

        // Typing characters goes into search query, not global shortcuts
        let _ = dispatch_key(&mut state, key(KeyCode::Char('r')));
        let _ = dispatch_key(&mut state, key(KeyCode::Char('a')));
        let _ = dispatch_key(&mut state, key(KeyCode::Char('d')));
        assert_eq!(state.search_query, "rad");

        // Submitting with query emits LoadPage and SaveSettings
        let effects = dispatch_key(&mut state, key(KeyCode::Enter));
        assert!(!state.text_entry, "Text entry should close on search submission");
        assert!(effects.contains(&Effect::SaveSettings));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::LoadPage { query, .. } if query == "rad"))
        );
    }

    #[test]
    fn search_input_down_and_up_arrow_navigation() {
        let mut state = AppState {
            recent_searches: vec!["arcade fire".into(), "radiohead".into()],
            ..AppState::default()
        };
        let _ = dispatch_key(&mut state, key(KeyCode::Char('2')));
        assert_eq!(state.page.route, Route::Search);
        assert!(state.text_entry);

        // Pressing Down leaves text entry and focuses content list at index 0
        let _ = dispatch_key(&mut state, key(KeyCode::Down));
        assert!(!state.text_entry);
        assert_eq!(state.focus, FocusRegion::Content);
        assert_eq!(state.page.cursor.selected, 0);

        // Pressing Down again moves cursor to index 1
        let _ = dispatch_key(&mut state, key(KeyCode::Down));
        assert_eq!(state.page.cursor.selected, 1);

        // Pressing Up moves cursor to index 0
        let _ = dispatch_key(&mut state, key(KeyCode::Up));
        assert_eq!(state.page.cursor.selected, 0);

        // Pressing Up at index 0 re-enters search text entry
        let _ = dispatch_key(&mut state, key(KeyCode::Up));
        assert!(state.text_entry);
    }

    #[test]
    fn search_submission_caches_recent_searches() {
        let mut state = AppState::default();
        state.page.route = Route::Search;
        state.search_query = "radiohead".into();

        let effects = submit_text(&mut state);
        assert_eq!(state.recent_searches, vec!["radiohead"]);
        assert!(effects.contains(&Effect::SaveSettings));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadPage { query, .. } if query == "radiohead"))
        );

        // Duplicate submission brings it to the top without duplicates
        state.search_query = "daft punk".into();
        let _ = submit_text(&mut state);
        assert_eq!(state.recent_searches, vec!["daft punk", "radiohead"]);

        state.search_query = "radiohead".into();
        let _ = submit_text(&mut state);
        assert_eq!(state.recent_searches, vec!["radiohead", "daft punk"]);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn recent_search_item_activation_executes_search() {
        let mut state = AppState::default();
        state.recent_searches = vec!["arcade fire".into()];
        state.populate_recent_searches();
        state.page.cursor.selected = 0;

        let effects = reduce(&mut state, Action::Activate);
        assert_eq!(state.search_query, "arcade fire");
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::LoadPage { query, .. } if query == "arcade fire"))
        );
    }

    #[test]
    fn page_cache_fast_path_avoids_redundant_load_effects() {
        let mut state = AppState::default();
        let home_page = PageState {
            route: Route::Home,
            title: "Home".into(),
            subtitle: "Cached".into(),
            artwork_url: None,
            uri: None,
            external_url: None,
            sections: Vec::new(),
            cursor: crate::state::ListCursor::default(),
            state: LoadState::Ready,
            filter: String::new(),
            library_tab: LibraryTab::Albums,
            settings_tab: SettingsTab::General,
            search_filter: SearchFilter::All,
            next_offset: None,
            loading_more: false,
            generation: 1,
        };
        state.page_cache.insert(Route::Home.key(), (home_page, Instant::now()));

        // Navigating to Home with fresh cache returns empty effects (no network round-trip)
        let effects = load_route(&mut state, Route::Home);
        assert!(effects.is_empty());
        assert_eq!(state.page.title, "Home");
        assert_eq!(state.page.subtitle, "Cached");
    }

    #[test]
    #[allow(clippy::too_many_lines, clippy::field_reassign_with_default)]
    fn detail_context_actions_dispatch_via_mouse_and_keyboard() {
        let mut state = AppState::default();
        state.page = PageState::loading(
            Route::Album { uri: "spotify:album:1".into(), title: "OK Computer".into() },
            1,
        );
        state.page.uri = Some("spotify:album:1".into());
        state.page.external_url = Some("https://open.spotify.com/album/1".into());
        state.selected_device_id = Some("dev-1".into());

        // Keyboard shortcuts
        let play_fx = dispatch_key(&mut state, key(KeyCode::Char('p')));
        assert_eq!(
            play_fx,
            vec![Effect::PlayTrack {
                uri: "spotify:album:1".into(),
                device_id: Some("dev-1".into()),
            }]
        );

        let shuffle_fx =
            dispatch_key(&mut state, KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert!(state.playback.shuffle);
        assert_eq!(
            shuffle_fx,
            vec![
                Effect::PlayTrack {
                    uri: "spotify:album:1".into(),
                    device_id: Some("dev-1".into()),
                },
                Effect::Shuffle(true),
            ]
        );

        // Shift+s (with lowercase 's' + SHIFT modifier)
        let shuffle_shift_s =
            dispatch_key(&mut state, KeyEvent::new(KeyCode::Char('s'), KeyModifiers::SHIFT));
        assert_eq!(
            shuffle_shift_s,
            vec![
                Effect::PlayTrack {
                    uri: "spotify:album:1".into(),
                    device_id: Some("dev-1".into()),
                },
                Effect::Shuffle(true),
            ]
        );

        // Plain 's' without SHIFT in detail route must toggle global shuffle, NOT restart context
        state.playback.shuffle = false;
        let toggle_shuffle = dispatch_key(&mut state, key(KeyCode::Char('s')));
        assert_eq!(toggle_shuffle, vec![Effect::Shuffle(true)]);

        // Artist detail route without explicit external_url derives web URL when 'o' pressed
        let mut artist_state = AppState::default();
        artist_state.page = PageState::loading(
            Route::Artist {
                uri: "spotify:artist:4Z8W4fKeB5YxbusRsdQVPb".into(),
                title: "Radiohead".into(),
            },
            1,
        );
        assert_eq!(artist_state.page.uri.as_deref(), Some("spotify:artist:4Z8W4fKeB5YxbusRsdQVPb"));
        assert_eq!(
            artist_state.page.external_url.as_deref(),
            Some("https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb")
        );
        let artist_spotify_fx = dispatch_key(&mut artist_state, key(KeyCode::Char('o')));
        assert_eq!(
            artist_spotify_fx,
            vec![Effect::OpenExternal(
                "https://open.spotify.com/artist/4Z8W4fKeB5YxbusRsdQVPb".into()
            )]
        );

        let spotify_fx = dispatch_key(&mut state, key(KeyCode::Char('o')));
        assert_eq!(
            spotify_fx,
            vec![Effect::OpenExternal("https://open.spotify.com/album/1".into())]
        );

        // Mouse click targets
        let mut hits = HitMap::default();
        hits.add(Rect { x: 10, y: 5, width: 10, height: 1 }, HitTarget::DetailPlay);
        hits.add(Rect { x: 22, y: 5, width: 10, height: 1 }, HitTarget::DetailShuffle);
        hits.add(Rect { x: 34, y: 5, width: 10, height: 1 }, HitTarget::DetailOpenSpotify);

        let click = |col| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: col,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };

        let mouse_play = dispatch_mouse(&mut state, &mut hits, click(12), Instant::now());
        assert_eq!(
            mouse_play,
            vec![Effect::PlayTrack {
                uri: "spotify:album:1".into(),
                device_id: Some("dev-1".into()),
            }]
        );

        let mouse_shuffle = dispatch_mouse(&mut state, &mut hits, click(24), Instant::now());
        assert_eq!(
            mouse_shuffle,
            vec![
                Effect::PlayTrack {
                    uri: "spotify:album:1".into(),
                    device_id: Some("dev-1".into()),
                },
                Effect::Shuffle(true),
            ]
        );

        let mouse_spotify = dispatch_mouse(&mut state, &mut hits, click(36), Instant::now());
        assert_eq!(
            mouse_spotify,
            vec![Effect::OpenExternal("https://open.spotify.com/album/1".into())]
        );
    }

    #[test]
    fn navigation_shortcut_five_navigates_to_queue() {
        let mut state = AppState::default();
        let _ = dispatch_key(&mut state, key(KeyCode::Char('5')));
        assert_eq!(state.page.route, Route::Queue);
    }

    #[test]
    fn activate_setting_cycles_adaptive_layout_thresholds_and_recomputes_layout() {
        let mut state = AppState { terminal_size: (110, 26), ..Default::default() };
        state.update_layout(110, 26);
        assert_eq!(state.layout, crate::state::LayoutMode::SidePlayer);

        // Populate setting items in page state
        state.page.sections = vec![crate::state::Section {
            title: "Terminal".into(),
            items: vec![
                crate::state::BrowseItem {
                    id: "side-player-height".into(),
                    kind: EntityKind::Action,
                    title: String::new(),
                    subtitle: String::new(),
                    metadata: String::new(),
                    uri: None,
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: true,
                    saved: None,
                    context: None,
                    restricted: false,
                },
                crate::state::BrowseItem {
                    id: "side-player-width".into(),
                    kind: EntityKind::Action,
                    title: String::new(),
                    subtitle: String::new(),
                    metadata: String::new(),
                    uri: None,
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: true,
                    saved: None,
                    context: None,
                    restricted: false,
                },
                crate::state::BrowseItem {
                    id: "stacked-queue-height".into(),
                    kind: EntityKind::Action,
                    title: String::new(),
                    subtitle: String::new(),
                    metadata: String::new(),
                    uri: None,
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: true,
                    saved: None,
                    context: None,
                    restricted: false,
                },
                crate::state::BrowseItem {
                    id: "wide-breakpoint".into(),
                    kind: EntityKind::Action,
                    title: String::new(),
                    subtitle: String::new(),
                    metadata: String::new(),
                    uri: None,
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: true,
                    saved: None,
                    context: None,
                    restricted: false,
                },
            ],
        }];

        // Cycle side-player-height from 28 -> 30
        let effects = activate_setting(&mut state, "side-player-height");
        assert_eq!(state.side_player_max_height, 30);
        assert!(effects.contains(&Effect::SaveSettings));

        // Cycle to 32 -> 0 (Disabled) -> 24 -> 26 -> 28
        activate_setting(&mut state, "side-player-height");
        assert_eq!(state.side_player_max_height, 32);
        activate_setting(&mut state, "side-player-height");
        assert_eq!(state.side_player_max_height, 0);
        // With side-player-max-height = 0, layout recomputes from SidePlayer to Standard
        assert_eq!(state.layout, crate::state::LayoutMode::Standard);

        // Cycle side-player-width: 100 -> 110 -> 120 -> 80 -> 90 -> 100
        activate_setting(&mut state, "side-player-width");
        assert_eq!(state.side_player_min_width, 110);

        // Cycle stacked-queue-height: 38 -> 42 -> 46 -> 999 -> 34 -> 38
        activate_setting(&mut state, "stacked-queue-height");
        assert_eq!(state.stacked_queue_min_height, 42);

        // Cycle wide-breakpoint: 140 -> 150 -> 160 -> 120 -> 130 -> 140
        activate_setting(&mut state, "wide-breakpoint");
        assert_eq!(state.wide_breakpoint_width, 150);
    }

    #[test]
    fn setting_change_persists_in_page_cache_on_subsequent_navigation() {
        let mut state = AppState::default();
        let generation = state.navigate(Route::Settings);
        let mut settings_page = PageState::loading(Route::Settings, generation);
        settings_page.sections = vec![crate::state::Section {
            title: "Terminal".into(),
            items: vec![crate::state::BrowseItem {
                id: "side-player-height".into(),
                kind: EntityKind::Action,
                title: "Side player max height: 28 rows".into(),
                subtitle: String::new(),
                metadata: String::new(),
                uri: None,
                external_url: None,
                artwork_url: None,
                artists: Vec::new(),
                album: None,
                duration_ms: None,
                available: true,
                saved: None,
                context: None,
                restricted: false,
            }],
        }];
        assert!(state.accept_page(settings_page));
        assert_eq!(state.page.sections[0].items[0].title, "Side player max height: 28 rows");

        // Activate setting: 28 -> 30
        activate_setting(&mut state, "side-player-height");
        assert_eq!(state.page.sections[0].items[0].title, "Side player max height: 30 rows");

        // Navigate to Home
        load_route(&mut state, Route::Home);
        assert_eq!(state.page.route, Route::Home);

        // Navigate back to Settings: should retrieve updated page with 30 rows, not old 28
        load_route(&mut state, Route::Settings);
        assert_eq!(state.page.route, Route::Settings);
        assert_eq!(state.page.sections[0].items[0].title, "Side player max height: 30 rows");
    }

    #[test]
    fn toggle_saved_toggles_and_emits_effect() {
        let mut state = AppState::default();
        state.page.route = Route::Album { uri: "spotify:album:1".into(), title: "Album".into() };
        state.page.state = LoadState::Ready;
        state.page.sections = vec![Section {
            title: "Tracks".into(),
            items: vec![BrowseItem {
                id: "t1".into(),
                kind: EntityKind::Track,
                title: "Song 1".into(),
                subtitle: "Artist 1".into(),
                metadata: String::new(),
                uri: Some("spotify:track:1".into()),
                external_url: None,
                artwork_url: None,
                artists: Vec::new(),
                album: None,
                duration_ms: Some(180_000),
                available: true,
                saved: Some(false),
                context: None,
                restricted: false,
            }],
        }];
        state.queue_upcoming.push(BrowseItem {
            id: "t1".into(),
            kind: EntityKind::Track,
            title: "Song 1".into(),
            subtitle: "Artist 1".into(),
            metadata: String::new(),
            uri: Some("spotify:track:1".into()),
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: Some(180_000),
            available: true,
            saved: Some(false),
            context: None,
            restricted: false,
        });

        let effects = reduce(&mut state, Action::ToggleSaved);
        assert_eq!(state.page.sections[0].items[0].saved, Some(true));
        assert_eq!(state.queue_upcoming[0].saved, Some(true));
        assert_eq!(effects, vec![Effect::SetSaved { uri: "spotify:track:1".into(), saved: true }]);
    }

    #[test]
    fn toggle_saved_in_liked_songs_removes_track_and_stabilizes_cursor() {
        let mut state = AppState::default();
        state.page.route = Route::Library;
        state.page.library_tab = LibraryTab::LikedSongs;
        state.page.state = LoadState::Ready;
        state.page.sections = vec![Section {
            title: "Liked Songs".into(),
            items: vec![
                BrowseItem {
                    id: "t1".into(),
                    kind: EntityKind::Track,
                    title: "Song 1".into(),
                    subtitle: "Artist 1".into(),
                    metadata: String::new(),
                    uri: Some("spotify:track:1".into()),
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: Some(180_000),
                    available: true,
                    saved: Some(true),
                    context: None,
                    restricted: false,
                },
                BrowseItem {
                    id: "t2".into(),
                    kind: EntityKind::Track,
                    title: "Song 2".into(),
                    subtitle: "Artist 2".into(),
                    metadata: String::new(),
                    uri: Some("spotify:track:2".into()),
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: Some(180_000),
                    available: true,
                    saved: Some(true),
                    context: None,
                    restricted: false,
                },
            ],
        }];

        // Unlike the first song
        let effects = reduce(&mut state, Action::ToggleSaved);
        assert_eq!(state.page.sections[0].items.len(), 1);
        assert_eq!(state.page.sections[0].items[0].title, "Song 2");
        assert_eq!(state.page.cursor.selected, 0);
        assert_eq!(effects, vec![Effect::SetSaved { uri: "spotify:track:1".into(), saved: false }]);
    }

    #[test]
    fn cycle_local_switches_library_tabs() {
        let mut state = AppState::default();
        state.page.route = Route::Library;
        state.library_tab = LibraryTab::LikedSongs;

        let effects = reduce(&mut state, Action::LocalRight);
        assert_eq!(state.library_tab, LibraryTab::Albums);
        assert_eq!(
            effects,
            vec![Effect::LoadPage {
                route: Route::Library,
                generation: state.generation,
                offset: 0,
                query: String::new(),
                tab: Some(LibraryTab::Albums),
            }]
        );

        let effects = reduce(&mut state, Action::LocalRight);
        assert_eq!(state.library_tab, LibraryTab::Playlists);
        assert_eq!(
            effects,
            vec![Effect::LoadPage {
                route: Route::Library,
                generation: state.generation,
                offset: 0,
                query: String::new(),
                tab: Some(LibraryTab::Playlists),
            }]
        );
    }

    #[test]
    fn action_menu_and_playlist_picker_flows() {
        let mut state = AppState::default();
        state.page.route = Route::Home;
        state.page.state = LoadState::Ready;
        state.page.sections = vec![Section {
            title: "Tracks".into(),
            items: vec![BrowseItem {
                id: "t1".into(),
                kind: EntityKind::Track,
                title: "Song 1".into(),
                subtitle: "Artist 1".into(),
                metadata: String::new(),
                uri: Some("spotify:track:1".into()),
                external_url: None,
                artwork_url: None,
                artists: Vec::new(),
                album: None,
                duration_ms: Some(180_000),
                available: true,
                saved: Some(false),
                context: None,
                restricted: false,
            }],
        }];

        // Press 'm' / Action::OpenActions opens Overlay::Actions
        reduce(&mut state, Action::OpenActions);
        assert!(matches!(state.overlay, Some(Overlay::Actions { .. })));

        // Select AddToPlaylist action
        let actions = match &state.overlay {
            Some(Overlay::Actions { actions, .. }) => actions.clone(),
            _ => panic!("Expected Overlay::Actions"),
        };
        let add_to_playlist_idx = actions
            .iter()
            .position(|a| *a == ContextAction::AddToPlaylist)
            .expect("AddToPlaylist action missing");
        let effects = activate_action_menu(&mut state, add_to_playlist_idx, &actions);
        assert_eq!(
            effects,
            vec![Effect::LoadPlaylistsForPicker { pending_uri: "spotify:track:1".into() }]
        );
        assert!(matches!(state.overlay, Some(Overlay::PlaylistPicker { .. })));

        // Picker selection -> AddToPlaylist effect
        let playlists = vec![BrowseItem {
            id: "p1".into(),
            kind: EntityKind::Playlist,
            title: "Favorites".into(),
            subtitle: String::new(),
            metadata: String::new(),
            uri: Some("spotify:playlist:fav123".into()),
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: None,
            available: true,
            saved: None,
            context: None,
            restricted: false,
        }];
        let picker_effects = activate_playlist_picker(&mut state, 0, &playlists, "spotify:track:1");
        assert_eq!(
            picker_effects,
            vec![Effect::AddToPlaylist {
                playlist_id: "fav123".into(),
                track_uri: "spotify:track:1".into(),
            }]
        );
        assert!(state.overlay.is_none());
    }

    #[test]
    fn user_interaction_clears_startup_focus_pending() {
        let mut state = AppState::default();
        assert!(state.startup_focus_pending);
        let _ = dispatch_key(&mut state, KeyEvent::from(KeyCode::Char('k')));
        assert!(!state.startup_focus_pending);

        state.startup_focus_pending = true;
        let mut hits = HitMap::default();
        let mouse_event = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        let _ = dispatch_mouse(&mut state, &mut hits, mouse_event, Instant::now());
        assert!(!state.startup_focus_pending);
    }

    #[test]
    fn fresh_library_visit_loads_normally() {
        let mut state = AppState::default();
        let effects = load_route(&mut state, Route::Library);
        assert_eq!(state.page.route, Route::Library);
        assert_eq!(state.page.library_tab, LibraryTab::LikedSongs);
        assert_eq!(state.page.state, crate::state::LoadState::Loading);
        assert_eq!(
            effects,
            vec![Effect::LoadPage {
                route: Route::Library,
                generation: state.generation,
                offset: 0,
                query: String::new(),
                tab: Some(LibraryTab::LikedSongs),
            }]
        );
    }

    #[test]
    fn fresh_cached_library_tab_opens_immediately_without_another_loading_state() {
        let mut state = AppState::default();
        let mut cached_page = PageState::loading(Route::Library, 1);
        cached_page.state = crate::state::LoadState::Ready;
        cached_page.library_tab = LibraryTab::LikedSongs;
        cached_page.sections = vec![crate::state::Section {
            title: "Liked Songs".into(),
            items: vec![BrowseItem::message("item-1", "Track 1")],
        }];
        state.page_cache.insert("library:liked".into(), (cached_page, Instant::now()));

        let effects = load_route(&mut state, Route::Library);
        assert_eq!(state.page.route, Route::Library);
        assert_eq!(state.page.state, crate::state::LoadState::Ready);
        assert!(effects.is_empty());
    }

    #[test]
    fn stale_cached_library_tab_opens_immediately_and_then_refreshes() {
        let mut state = AppState::default();
        let mut cached_page = PageState::loading(Route::Library, 1);
        cached_page.state = crate::state::LoadState::Ready;
        cached_page.library_tab = LibraryTab::LikedSongs;
        cached_page.sections = vec![crate::state::Section {
            title: "Liked Songs".into(),
            items: vec![BrowseItem::message("item-1", "Track 1")],
        }];
        state.page_cache.insert(
            "library:liked".into(),
            (
                cached_page,
                Instant::now().checked_sub(Duration::from_secs(120)).unwrap_or_else(Instant::now),
            ),
        );

        let effects = load_route(&mut state, Route::Library);
        assert_eq!(state.page.route, Route::Library);
        assert_eq!(state.page.state, crate::state::LoadState::Ready);
        assert_eq!(
            effects,
            vec![Effect::LoadPage {
                route: Route::Library,
                generation: state.generation,
                offset: 0,
                query: String::new(),
                tab: Some(LibraryTab::LikedSongs),
            }]
        );

        let mut stale_albums = PageState::loading(Route::Library, 1);
        stale_albums.state = crate::state::LoadState::Ready;
        stale_albums.library_tab = LibraryTab::Albums;
        stale_albums.sections = vec![crate::state::Section {
            title: "Albums".into(),
            items: vec![BrowseItem::message("album-1", "Album 1")],
        }];
        state.page_cache.insert(
            "library:albums".into(),
            (
                stale_albums,
                Instant::now().checked_sub(Duration::from_secs(120)).unwrap_or_else(Instant::now),
            ),
        );

        let switch_effects = reduce(&mut state, Action::LocalRight);
        assert_eq!(state.library_tab, LibraryTab::Albums);
        assert_eq!(state.page.library_tab, LibraryTab::Albums);
        assert_eq!(state.page.state, crate::state::LoadState::Ready);
        assert_eq!(
            switch_effects,
            vec![Effect::LoadPage {
                route: Route::Library,
                generation: state.generation,
                offset: 0,
                query: String::new(),
                tab: Some(LibraryTab::Albums),
            }]
        );
    }

    #[test]
    fn switching_tabs_never_displays_content_from_wrong_library_tab() {
        let mut state = AppState::default();
        let mut liked_page = PageState::loading(Route::Library, 1);
        liked_page.state = crate::state::LoadState::Ready;
        liked_page.library_tab = LibraryTab::LikedSongs;
        let mut liked_item = BrowseItem::message("track-1", "Liked Track 1");
        liked_item.kind = EntityKind::Track;
        liked_page.sections =
            vec![crate::state::Section { title: "Liked Songs".into(), items: vec![liked_item] }];
        state.page_cache.insert("library:liked".into(), (liked_page, Instant::now()));

        let mut album_page = PageState::loading(Route::Library, 1);
        album_page.state = crate::state::LoadState::Ready;
        album_page.library_tab = LibraryTab::Albums;
        let mut album_item = BrowseItem::message("album-1", "Album 1");
        album_item.kind = EntityKind::Album;
        album_page.sections =
            vec![crate::state::Section { title: "Albums".into(), items: vec![album_item] }];
        state.page_cache.insert("library:albums".into(), (album_page, Instant::now()));

        load_route(&mut state, Route::Library);
        assert_eq!(state.page.library_tab, LibraryTab::LikedSongs);
        assert_eq!(state.page.flattened()[0].title, "Liked Track 1");

        reduce(&mut state, Action::LocalRight);
        assert_eq!(state.page.library_tab, LibraryTab::Albums);
        assert_eq!(state.page.flattened()[0].title, "Album 1");
    }

    #[test]
    fn activate_setting_toggles_artwork_behind_menus() {
        let mut state = AppState::default();
        assert!(!state.artwork_under_overlays);

        state.page.sections = vec![crate::state::Section {
            title: "Terminal".into(),
            items: vec![crate::state::BrowseItem {
                id: "artwork-under-overlays".into(),
                kind: EntityKind::Action,
                title: "Artwork behind menus: disabled".into(),
                subtitle: String::new(),
                metadata: String::new(),
                uri: None,
                external_url: None,
                artwork_url: None,
                artists: Vec::new(),
                album: None,
                duration_ms: None,
                available: true,
                saved: None,
                context: None,
                restricted: false,
            }],
        }];

        let effects = activate_setting(&mut state, "artwork-under-overlays");
        assert!(state.artwork_under_overlays);
        assert_eq!(state.page.sections[0].items[0].title, "Artwork behind menus: enabled");
        assert_eq!(effects, vec![Effect::SaveSettings]);

        let effects2 = activate_setting(&mut state, "artwork-under-overlays");
        assert!(!state.artwork_under_overlays);
        assert_eq!(state.page.sections[0].items[0].title, "Artwork behind menus: disabled");
        assert_eq!(effects2, vec![Effect::SaveSettings]);
    }

    #[test]
    fn custom_binding_triggers_action_and_old_stops() {
        let mut state = AppState::default();
        let space_key = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        let p_key = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE);

        assert_eq!(dispatch_key(&mut state, space_key), vec![Effect::TogglePlayback]);
        assert!(dispatch_key(&mut state, p_key).is_empty());

        state.shortcuts.force_assign(
            crate::shortcuts::ShortcutAction::TogglePlayback,
            crate::shortcuts::KeyBinding::parse("p").unwrap(),
        );

        assert_eq!(dispatch_key(&mut state, p_key), vec![Effect::TogglePlayback]);
        assert!(dispatch_key(&mut state, space_key).is_empty());
    }

    #[test]
    fn global_quit_works_in_text_entry_and_overlays() {
        let mut state = AppState::default();
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        let x_key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        let shift_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::SHIFT);
        let lower_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);

        // In text entry
        state.page.route = Route::Search;
        state.text_entry = true;
        dispatch_key(&mut state, ctrl_c);
        assert!(state.quit);

        // Printable quit keys are plain text while typing.
        state.quit = false;
        state.text_entry = true;
        state.search_query.clear();
        for key in [x_key, shift_q, lower_q] {
            let effects = dispatch_key(&mut state, key);
            assert!(effects.is_empty());
            assert!(!state.quit);
        }
        assert_eq!(state.search_query, "xqq");

        // In an overlay
        state.text_entry = false;
        state.overlay = Some(crate::state::Overlay::Help { query: String::new(), editing: false });
        dispatch_key(&mut state, ctrl_c);
        assert!(state.quit);
    }

    #[test]
    fn plus_and_minus_perform_volume_controls() {
        let mut state = AppState::default();
        let initial_volume = state.playback.volume;
        let plus = KeyEvent::new(KeyCode::Char('+'), KeyModifiers::NONE);
        let minus = KeyEvent::new(KeyCode::Char('-'), KeyModifiers::NONE);

        let effects_plus = dispatch_key(&mut state, plus);
        assert_eq!(effects_plus, vec![Effect::Volume(initial_volume.saturating_add(5).min(100))]);

        let effects_minus = dispatch_key(&mut state, minus);
        assert_eq!(effects_minus, vec![Effect::Volume(initial_volume)]);
    }

    #[test]
    fn shortcut_capture_and_reassignment_flow() {
        let mut state = AppState::default();
        let effects = activate_setting(&mut state, "shortcut:toggle_playback");
        assert!(effects.is_empty());
        assert!(matches!(
            state.overlay,
            Some(crate::state::Overlay::ShortcutCapture {
                action: crate::shortcuts::ShortcutAction::TogglePlayback,
                conflict: None,
            })
        ));

        let bracket = KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE);
        assert!(dispatch_key(&mut state, bracket).is_empty());
        assert!(matches!(
            state.overlay,
            Some(crate::state::Overlay::ShortcutCapture {
                action: crate::shortcuts::ShortcutAction::TogglePlayback,
                conflict: Some(_),
            })
        ));

        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let save_effects = dispatch_key(&mut state, enter);
        assert_eq!(save_effects, vec![Effect::SaveSettings]);
        assert!(state.overlay.is_none());
        assert_eq!(
            state.shortcuts.primary_canonical(crate::shortcuts::ShortcutAction::TogglePlayback),
            "]"
        );
        assert!(
            state.shortcuts.get_bindings(crate::shortcuts::ShortcutAction::NextTrack).is_empty()
        );
    }

    #[test]
    fn shortcut_capture_respects_configured_quit_cancel_activate() {
        let mut state = AppState::default();

        // Remap Cancel from Esc to 'z', and Activate from Enter to 'a'
        state.shortcuts.force_assign(
            crate::shortcuts::ShortcutAction::Cancel,
            crate::shortcuts::KeyBinding::parse("z").unwrap(),
        );
        state.shortcuts.force_assign(
            crate::shortcuts::ShortcutAction::Activate,
            crate::shortcuts::KeyBinding::parse("a").unwrap(),
        );

        // 1. In capture mode, pressing configured Cancel ('z') cancels and closes overlay
        state.overlay = Some(crate::state::Overlay::ShortcutCapture {
            action: crate::shortcuts::ShortcutAction::TogglePlayback,
            conflict: None,
        });
        let z_key = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE);
        let effects = dispatch_key(&mut state, z_key);
        assert!(effects.is_empty());
        assert!(state.overlay.is_none());
        assert_ne!(
            state.shortcuts.get_bindings(crate::shortcuts::ShortcutAction::TogglePlayback),
            &[crate::shortcuts::KeyBinding::parse("z").unwrap()]
        );

        // 2. In conflict mode, pressing configured Cancel ('z') cancels without reassignment
        let new_binding = crate::shortcuts::KeyBinding::parse("]").unwrap();
        state.overlay = Some(crate::state::Overlay::ShortcutCapture {
            action: crate::shortcuts::ShortcutAction::TogglePlayback,
            conflict: Some((new_binding, crate::shortcuts::ShortcutAction::NextTrack)),
        });
        let effects = dispatch_key(&mut state, z_key);
        assert!(effects.is_empty());
        assert!(state.overlay.is_none());
        assert_ne!(
            state.shortcuts.get_bindings(crate::shortcuts::ShortcutAction::TogglePlayback),
            &[new_binding]
        );

        // 3. In conflict mode, pressing configured Activate ('a') confirms reassignment
        state.overlay = Some(crate::state::Overlay::ShortcutCapture {
            action: crate::shortcuts::ShortcutAction::TogglePlayback,
            conflict: Some((new_binding, crate::shortcuts::ShortcutAction::NextTrack)),
        });
        let a_key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        let save_effects = dispatch_key(&mut state, a_key);
        assert_eq!(save_effects, vec![Effect::SaveSettings]);
        assert!(state.overlay.is_none());
        assert_eq!(
            state.shortcuts.get_bindings(crate::shortcuts::ShortcutAction::TogglePlayback),
            &[new_binding]
        );

        // 4. Global Quit works during capture mode
        state.overlay = Some(crate::state::Overlay::ShortcutCapture {
            action: crate::shortcuts::ShortcutAction::TogglePlayback,
            conflict: None,
        });
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        dispatch_key(&mut state, ctrl_c);
        assert!(state.quit);
    }

    #[test]
    fn text_entry_respects_configured_activate_and_cancel() {
        let mut state = AppState::default();

        // Remap Cancel to Ctrl+X and Activate to Ctrl+O
        let cancel_key =
            crate::shortcuts::KeyBinding::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        let activate_key =
            crate::shortcuts::KeyBinding::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        state.shortcuts.force_assign(crate::shortcuts::ShortcutAction::Cancel, cancel_key);
        state.shortcuts.force_assign(crate::shortcuts::ShortcutAction::Activate, activate_key);

        // 1. Rename Playlist overlay: Cancel closes overlay
        state.overlay = Some(crate::state::Overlay::RenamePlaylist {
            playlist_id: "p1".into(),
            name: "Initial".into(),
        });
        let ctrl_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        dispatch_key(&mut state, ctrl_x);
        assert!(state.overlay.is_none());

        // 2. Rename Playlist overlay: typing characters and submitting with Activate
        state.overlay = Some(crate::state::Overlay::RenamePlaylist {
            playlist_id: "p1".into(),
            name: "My".into(),
        });
        dispatch_key(&mut state, KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        dispatch_key(&mut state, KeyEvent::new(KeyCode::Char('P'), KeyModifiers::NONE));
        let ctrl_o = KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL);
        let effects = dispatch_key(&mut state, ctrl_o);
        assert_eq!(
            effects,
            vec![Effect::RenamePlaylist { playlist_id: "p1".into(), new_name: "My P".into() }]
        );
        assert!(state.overlay.is_none());

        // 3. Help overlay: editing mode stops editing on Activate and closes on Cancel
        state.overlay = Some(crate::state::Overlay::Help { query: "vol".into(), editing: true });
        state.text_entry = true;
        dispatch_key(&mut state, ctrl_o);
        assert!(matches!(state.overlay, Some(crate::state::Overlay::Help { editing: false, .. })));
        assert!(!state.text_entry);

        // Cancel closes Help overlay
        dispatch_key(&mut state, ctrl_x);
        assert!(state.overlay.is_none());
    }
}
