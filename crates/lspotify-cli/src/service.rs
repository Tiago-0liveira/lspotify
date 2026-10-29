use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

use lspotify_core::{AppError, ErrorKind, Result, SpotifyItemKind, SpotifyUri};
use lspotify_spotify::{
    SpotifyBrowseItem, SpotifyContentState, SpotifyDetailPage, SpotifyDisplayItem,
    SpotifyEntityKind, SpotifyPlayback, SpotifyTypedQueue, SpotifyWebApi,
};

use crate::{
    Effect,
    state::{
        BrowseItem, ContextPosition, EntityKind, LibraryTab, LoadState, PageState, Route, Section,
    },
};

const PAGE_SIZE: u32 = 50;

#[derive(Debug)]
pub enum ServiceResponse {
    Page(PageState),
    Playback { playback: SpotifyPlayback, generation: u64 },
    Queue { now: Option<BrowseItem>, upcoming: Vec<BrowseItem> },
    PlaylistPicker { playlists: Vec<BrowseItem>, pending_uri: String },
    Command { effect: Effect, result: Result<()> },
}

#[derive(Debug)]
enum Request {
    Effect(Effect, u64),
    Token(String),
    Shutdown,
}

#[derive(Debug)]
pub struct ServiceHandle {
    priority: SyncSender<Request>,
    browse: SyncSender<Request>,
    responses: Receiver<ServiceResponse>,
    worker: Option<thread::JoinHandle<()>>,
    latest_browse_generation: Arc<AtomicU64>,
    shutdown: Arc<AtomicBool>,
    playback_generation: AtomicU64,
    pending_polls: Arc<PendingPolls>,
}

#[derive(Debug, Default)]
struct PendingPolls {
    playback: AtomicBool,
    queue: AtomicBool,
}

impl PendingPolls {
    fn flag(&self, effect: &Effect) -> Option<&AtomicBool> {
        match effect {
            Effect::RefreshPlayback => Some(&self.playback),
            Effect::RefreshQueue => Some(&self.queue),
            _ => None,
        }
    }

    fn clear(&self, response: &ServiceResponse) {
        let flag = match response {
            ServiceResponse::Playback { .. } => Some(&self.playback),
            ServiceResponse::Queue { .. } => Some(&self.queue),
            ServiceResponse::Command { effect, .. } => self.flag(effect),
            _ => None,
        };
        if let Some(flag) = flag {
            flag.store(false, Ordering::Release);
        }
    }
}

impl ServiceHandle {
    /// Starts the serialized Spotify background worker.
    ///
    /// # Errors
    /// Returns an error if the HTTP client or worker thread cannot be created.
    pub fn start(access_token: String) -> Result<Self> {
        let api = SpotifyWebApi::new()?;
        Self::start_with_performer(access_token, move |token, effect| perform(&api, token, effect))
    }

    pub(crate) fn start_with_performer<F>(access_token: String, performer: F) -> Result<Self>
    where
        F: FnMut(&str, Effect) -> ServiceResponse + Send + 'static,
    {
        Self::start_with_performer_and_response_capacity(access_token, 64, performer)
    }

    pub(crate) fn start_with_performer_and_response_capacity<F>(
        access_token: String,
        response_capacity: usize,
        performer: F,
    ) -> Result<Self>
    where
        F: FnMut(&str, Effect) -> ServiceResponse + Send + 'static,
    {
        let (priority_tx, priority_rx) = mpsc::sync_channel(64);
        let (browse_tx, browse_rx) = mpsc::sync_channel(8);
        let (response_tx, response_rx) = mpsc::sync_channel(response_capacity);
        let latest_browse_generation = Arc::new(AtomicU64::new(0));
        let worker_generation = Arc::clone(&latest_browse_generation);
        let shutdown = Arc::new(AtomicBool::new(false));
        let worker_shutdown = Arc::clone(&shutdown);
        let pending_polls = Arc::new(PendingPolls::default());
        let worker_polls = Arc::clone(&pending_polls);
        let worker = thread::Builder::new()
            .name("lspotify-spotify".into())
            .spawn(move || {
                worker_loop(
                    performer,
                    access_token,
                    &priority_rx,
                    &browse_rx,
                    &response_tx,
                    &worker_generation,
                    &worker_shutdown,
                    &worker_polls,
                );
            })
            .map_err(|error| {
                AppError::new(
                    ErrorKind::Unavailable,
                    format!("could not start Spotify worker: {error}"),
                )
            })?;
        Ok(Self {
            priority: priority_tx,
            browse: browse_tx,
            responses: response_rx,
            worker: Some(worker),
            latest_browse_generation,
            shutdown,
            playback_generation: AtomicU64::new(0),
            pending_polls,
        })
    }

    /// Queues a browse, polling, or playback effect.
    ///
    /// # Errors
    /// Returns an error if the bounded queue is full or the worker stopped.
    pub fn send(&self, effect: Effect) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(AppError::new(ErrorKind::Unavailable, "worker is shutting down"));
        }
        if let Effect::LoadPage { generation, .. } = &effect {
            self.latest_browse_generation.fetch_max(*generation, Ordering::Release);
        }
        let priority = matches!(
            effect,
            Effect::TogglePlayback
                | Effect::Previous
                | Effect::Next
                | Effect::Seek(_)
                | Effect::Volume(_)
                | Effect::Shuffle(_)
                | Effect::Repeat(_)
                | Effect::PlayTrack { .. }
                | Effect::PlayContext { .. }
                | Effect::PlayTracks { .. }
                | Effect::Enqueue(_)
                | Effect::Transfer(_)
                | Effect::TransferLocal(_)
                | Effect::RefreshPlayback
                | Effect::RefreshQueue
                | Effect::SetSaved { .. }
                | Effect::AddToPlaylist { .. }
                | Effect::RemoveFromPlaylist { .. }
                | Effect::RenamePlaylist { .. }
        );
        let pending = self.pending_polls.flag(&effect);
        if pending.is_some_and(|flag| flag.swap(true, Ordering::AcqRel)) {
            return Ok(());
        }
        let sender = if priority { &self.priority } else { &self.browse };
        let result = sender.try_send(Request::Effect(effect, self.playback_generation()));
        let dropped_poll = pending.is_some() && matches!(result, Err(mpsc::TrySendError::Full(_)));
        if result.is_err()
            && let Some(flag) = pending
        {
            flag.store(false, Ordering::Release);
        }
        if dropped_poll {
            return Ok(());
        }
        result.map_err(|error| {
            AppError::new(ErrorKind::Unavailable, format!("background queue is busy: {error}"))
        })
    }

    /// Replaces the access token used by subsequent requests.
    ///
    /// # Errors
    /// Returns an error if the worker stopped.
    pub fn update_token(&self, token: String) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(AppError::new(ErrorKind::Unavailable, "worker is shutting down"));
        }
        self.priority.try_send(Request::Token(token)).map_err(channel_error)
    }

    pub fn mark_local_mutation(&self) {
        self.playback_generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn playback_generation(&self) -> u64 {
        self.playback_generation.load(Ordering::Acquire)
    }

    pub fn try_recv(&self) -> Option<ServiceResponse> {
        let response = self.responses.try_recv().ok()?;
        self.pending_polls.clear(&response);
        Some(response)
    }

    pub fn latest_browse_generation(&self) -> u64 {
        self.latest_browse_generation.load(Ordering::Acquire)
    }

    pub fn set_latest_browse_generation(&self, generation: u64) {
        self.latest_browse_generation.store(generation, Ordering::Release);
    }

    /// Requests shutdown of the worker thread without blocking.
    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.priority.try_send(Request::Shutdown);
    }

    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub fn join_worker(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        self.request_shutdown();
        drop(self.worker.take());
    }
}

#[allow(clippy::needless_pass_by_value, clippy::too_many_arguments)]
fn worker_loop<F>(
    mut performer: F,
    mut token: String,
    priority: &Receiver<Request>,
    browse: &Receiver<Request>,
    responses: &SyncSender<ServiceResponse>,
    latest_browse_generation: &AtomicU64,
    shutdown: &AtomicBool,
    pending_polls: &PendingPolls,
) where
    F: FnMut(&str, Effect) -> ServiceResponse,
{
    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        let request = match priority.try_recv() {
            Ok(request) => request,
            Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => match browse.recv_timeout(Duration::from_millis(50)) {
                Ok(request) => request,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            },
        };
        if shutdown.load(Ordering::Acquire) {
            break;
        }
        match request {
            Request::Shutdown => break,
            Request::Token(value) => token = value,
            Request::Effect(effect, generation) => {
                if is_obsolete_browse_effect(
                    &effect,
                    latest_browse_generation.load(Ordering::Acquire),
                ) {
                    continue;
                }
                let response = match performer(&token, effect) {
                    ServiceResponse::Playback { playback, .. } => {
                        ServiceResponse::Playback { playback, generation }
                    }
                    response => response,
                };
                if shutdown.load(Ordering::Acquire) {
                    break;
                }
                match responses.try_send(response) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(response)) => {
                        pending_polls.clear(&response);
                        if shutdown.load(Ordering::Acquire) {
                            break;
                        }
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
                }
            }
        }
    }
}

pub(crate) fn is_obsolete_browse_effect(effect: &Effect, latest_generation: u64) -> bool {
    match effect {
        Effect::LoadPage { generation, .. } => *generation < latest_generation,
        _ => false,
    }
}

fn poll_response(api: &SpotifyWebApi, token: &str, effect: Effect) -> ServiceResponse {
    match effect {
        Effect::RefreshPlayback => match api.playback(token) {
            Ok(playback) => ServiceResponse::Playback { playback, generation: 0 },
            Err(error) => ServiceResponse::Command { effect, result: Err(error) },
        },
        Effect::RefreshQueue => match api.typed_queue(token) {
            Ok(queue) => queue_response(queue),
            Err(error) => ServiceResponse::Command { effect, result: Err(error) },
        },
        _ => unreachable!("only polling effects are routed here"),
    }
}

fn perform(api: &SpotifyWebApi, token: &str, effect: Effect) -> ServiceResponse {
    match effect.clone() {
        Effect::LoadPage { route, generation, offset, query, tab } => {
            match load_page(api, token, route, generation, offset, &query, tab) {
                Ok(page) => ServiceResponse::Page(page),
                Err(error) => ServiceResponse::Command { effect, result: Err(error) },
            }
        }
        Effect::RefreshPlayback | Effect::RefreshQueue => poll_response(api, token, effect),
        Effect::TogglePlayback => {
            let result = api.playback(token).and_then(|snapshot| {
                if snapshot.playing { api.pause(token) } else { api.resume(token) }
            });
            ServiceResponse::Command { effect, result }
        }
        Effect::Previous => ServiceResponse::Command { effect, result: api.previous(token) },
        Effect::Next => ServiceResponse::Command { effect, result: api.next(token) },
        Effect::Seek(delta) => {
            let result = api.playback(token).and_then(|snapshot| {
                let current = i64::try_from(snapshot.progress_ms).unwrap_or(i64::MAX);
                api.seek(token, current.saturating_add(delta).max(0).cast_unsigned())
            });
            ServiceResponse::Command { effect, result }
        }
        Effect::Volume(value) => {
            ServiceResponse::Command { effect, result: api.volume(token, value) }
        }
        Effect::Shuffle(value) => {
            ServiceResponse::Command { effect, result: api.shuffle(token, value) }
        }
        Effect::Repeat(value) => {
            ServiceResponse::Command { effect, result: api.repeat(token, repeat_mode(value)) }
        }
        Effect::PlayTrack { uri, device_id } => {
            let result = play_track_with_retry(api, token, &uri, device_id.as_deref());
            ServiceResponse::Command { effect, result }
        }
        Effect::PlayContext { uri, position, device_id } => {
            let result = play_context_with_retry(api, token, &uri, position, device_id.as_deref());
            ServiceResponse::Command { effect, result }
        }
        Effect::PlayTracks { uris, device_id } => ServiceResponse::Command {
            effect,
            result: play_tracks_with_retry(api, token, &uris, device_id.as_deref()),
        },
        Effect::Enqueue(uri) => {
            ServiceResponse::Command { effect, result: api.enqueue(token, &uri) }
        }
        Effect::Transfer(id) => {
            ServiceResponse::Command { effect, result: api.transfer(token, &id) }
        }
        Effect::TransferLocal(id) => {
            let result = transfer_local_when_registered(api, token, &id);
            ServiceResponse::Command { effect, result }
        }
        Effect::SetSaved { uri, saved } => {
            let result = if saved {
                api.save_library_items(token, &[&uri])
            } else {
                api.remove_library_items(token, &[&uri])
            };
            ServiceResponse::Command { effect, result }
        }
        Effect::AddToPlaylist { playlist_id, track_uri } => {
            let result = api.add_to_playlist(token, &playlist_id, &[&track_uri]).map(|_| ());
            ServiceResponse::Command { effect, result }
        }
        Effect::RemoveFromPlaylist { playlist_uri, track_uri } => {
            let playlist_id =
                playlist_uri.strip_prefix("spotify:playlist:").unwrap_or(&playlist_uri);
            let result = api.remove_from_playlist(token, playlist_id, &[&track_uri]).map(|_| ());
            ServiceResponse::Command { effect, result }
        }
        Effect::RenamePlaylist { playlist_id, new_name } => {
            let result = api.update_playlist(token, &playlist_id, &new_name);
            ServiceResponse::Command { effect, result }
        }
        Effect::LoadPlaylistsForPicker { pending_uri } => {
            match api.playlists_page(token, 0, PAGE_SIZE) {
                Ok(page) => {
                    let playlists =
                        page.items.into_iter().map(|item| typed_item(item, None)).collect();
                    ServiceResponse::PlaylistPicker { playlists, pending_uri }
                }
                Err(error) => ServiceResponse::Command { effect, result: Err(error) },
            }
        }
        Effect::StartLocalPlayback
        | Effect::ChangeLocalPlaybackBackend(_)
        | Effect::OpenExternal(_)
        | Effect::SaveSettings => ServiceResponse::Command { effect, result: Ok(()) },
        Effect::WarmLikedSongs => {
            match load_page(api, token, Route::Library, 0, 0, "", Some(LibraryTab::LikedSongs)) {
                Ok(page) => ServiceResponse::Page(page),
                Err(error) => ServiceResponse::Command { effect, result: Err(error) },
            }
        }
    }
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn load_page(
    api: &SpotifyWebApi,
    token: &str,
    route: Route,
    generation: u64,
    offset: u32,
    query: &str,
    tab: Option<LibraryTab>,
) -> Result<PageState> {
    let mut page = PageState::loading(route.clone(), generation);
    match &route {
        Route::Home => {
            let home = api.load_home(token)?;
            page.title = "Home".into();
            page.subtitle = "Your listening, kept in distinct collections".into();
            page.sections = vec![
                display_section("Recently played", home.recently_played.items),
                display_section("Top tracks", home.top_tracks.items),
                display_section("Top artists", home.top_artists.items),
                display_section("Saved albums", home.saved_albums.items),
                display_section("Playlists", home.playlists.items),
            ];
            let warnings = [
                home.recently_played.warning,
                home.top_tracks.warning,
                home.top_artists.warning,
                home.saved_albums.warning,
                home.playlists.warning,
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
            page.state = if warnings.is_empty() {
                ready_or_empty(&page)
            } else {
                LoadState::Partial(warnings.join(" · "))
            };
        }
        Route::Library => {
            let active_tab = tab.unwrap_or(LibraryTab::LikedSongs);
            page.library_tab = active_tab;
            match active_tab {
                LibraryTab::LikedSongs => {
                    let liked = api.liked_tracks_page(token, offset, PAGE_SIZE)?;
                    page.title = "Library".into();
                    page.subtitle = "Liked Songs".into();
                    page.next_offset = liked.next_offset;
                    let items = liked
                        .items
                        .into_iter()
                        .map(|item| {
                            let mut bi = typed_item(item, Some("spotify:collection:tracks"));
                            bi.saved = Some(true);
                            bi
                        })
                        .collect();
                    page.sections = vec![Section { title: "Liked Songs".into(), items }];
                    page.state = ready_or_empty(&page);
                }
                LibraryTab::Albums => {
                    let albums = api.saved_albums_page(token, offset, PAGE_SIZE)?;
                    page.title = "Library".into();
                    page.subtitle = "Albums".into();
                    page.next_offset = albums.next_offset;
                    page.sections = vec![typed_section("Albums", albums.items, None)];
                    page.state = ready_or_empty(&page);
                }
                LibraryTab::Playlists => {
                    let playlists = api.playlists_page(token, offset, PAGE_SIZE)?;
                    page.title = "Library".into();
                    page.subtitle = "Playlists".into();
                    page.next_offset = playlists.next_offset;
                    page.sections = vec![typed_section("Playlists", playlists.items, None)];
                    page.state = ready_or_empty(&page);
                }
            }
        }
        Route::Search => {
            page.title = "Search".into();
            page.subtitle = "Press Enter to search Spotify".into();
            if query.trim().is_empty() {
                page.state = LoadState::Empty("Type a query, then press Enter.".into());
            } else {
                let results = api.search_page(token, query, offset, PAGE_SIZE)?;
                page.next_offset = results.next_offset;
                page.sections = vec![typed_section("Results", results.items, None)];
                page.state = ready_or_empty(&page);
                batch_populate_saved_tracks(api, token, &mut page);
            }
        }
        Route::Album { uri, .. } => {
            apply_detail(&mut page, api.album_page(token, uri)?);
            batch_populate_saved_tracks(api, token, &mut page);
        }
        Route::Playlist { uri, .. } => {
            apply_detail(&mut page, api.playlist_page(token, uri, offset, PAGE_SIZE)?);
            batch_populate_saved_tracks(api, token, &mut page);
        }
        Route::Artist { uri, .. } => {
            apply_detail(&mut page, api.artist_page(token, uri)?);
            batch_populate_saved_tracks(api, token, &mut page);
        }
        Route::Queue => {
            let queue = api.queue(token)?;
            page.title = "Queue".into();
            page.subtitle = "Now playing and server-ordered upcoming tracks".into();
            let mut sections = vec![Section {
                title: "Now playing".into(),
                items: queue
                    .current
                    .into_iter()
                    .map(|item| display_item(item, Some(EntityKind::Track)))
                    .collect(),
            }];
            sections.push(Section {
                title: "Up next".into(),
                items: queue
                    .upcoming
                    .into_iter()
                    .map(|item| display_item(item, Some(EntityKind::Track)))
                    .collect(),
            });
            page.sections = sections;
            page.state = ready_or_empty(&page);
            batch_populate_saved_tracks(api, token, &mut page);
        }
        Route::Devices => {
            page.title = "Devices".into();
            page.subtitle = "Choose local playback or any Spotify Connect device".into();
            let local = BrowseItem {
                id: crate::action::LOCAL_DEVICE_PLACEHOLDER.into(),
                kind: EntityKind::Device,
                title: "This computer".into(),
                subtitle: "Local playback on this computer".into(),
                metadata: "Uses the local playback engine configured in Settings.".into(),
                uri: Some(crate::action::LOCAL_DEVICE_PLACEHOLDER.into()),
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
            let remote = api
                .devices(token)?
                .into_iter()
                .filter(|device| device.name != "lspotify")
                .map(|device| BrowseItem {
                    id: device.id.clone(),
                    kind: EntityKind::Device,
                    title: format!(
                        "{}{}",
                        device.name,
                        if device.active { " · active" } else { "" }
                    ),
                    subtitle: if device.restricted {
                        format!("{} · restricted", device.kind)
                    } else {
                        device.kind
                    },
                    metadata: if device.restricted {
                        "Spotify reports that this device cannot accept remote commands.".into()
                    } else {
                        "Enter selects this device for playback.".into()
                    },
                    uri: Some(device.id),
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: !device.restricted,
                    saved: None,
                    context: None,
                    restricted: device.restricted,
                })
                .collect();
            page.sections = vec![
                Section { title: "Local".into(), items: vec![local] },
                Section { title: "Spotify Connect".into(), items: remote },
            ];
            page.state = ready_or_empty(&page);
        }
        Route::Settings => {
            page.title = "Settings".into();
            page.subtitle = "CLI appearance and interaction".into();
            page.sections = vec![Section {
                title: "Terminal".into(),
                items: vec![
                    setting_item("artwork", "Artwork: Auto / Blocks / Off"),
                    setting_item("artwork-under-overlays", "Artwork behind menus: disabled"),
                    setting_item("mouse", "Mouse input: enabled"),
                    setting_item("wide-queue", "Wide-screen queue: enabled"),
                    setting_item("side-player-height", "Side player max height: 28 rows"),
                    setting_item("side-player-width", "Side player min width: 100 cols"),
                    setting_item("stacked-queue-height", "Stacked queue min height: 38 rows"),
                    setting_item("wide-breakpoint", "Wide layout min width: 140 cols"),
                ],
            }];
            page.state = LoadState::Ready;
        }
    }
    Ok(page)
}

fn apply_detail(page: &mut PageState, detail: SpotifyDetailPage) {
    page.subtitle = detail_line(&detail);
    page.title.clone_from(&detail.title);
    page.artwork_url.clone_from(&detail.artwork_url);
    let context = matches!(
        detail.kind,
        lspotify_spotify::SpotifyDetailKind::Album | lspotify_spotify::SpotifyDetailKind::Playlist
    )
    .then_some(detail.uri.clone());
    let primary = match detail.kind {
        lspotify_spotify::SpotifyDetailKind::Artist => "Songs (search-derived)",
        _ => "Tracks",
    };
    page.uri = Some(detail.uri.clone());
    page.external_url = detail
        .external_url
        .or_else(|| detail.uri.parse::<SpotifyUri>().ok().map(|parsed| parsed.web_url()));
    page.next_offset = detail.items.next_offset;
    page.sections = vec![typed_section(primary, detail.items.items, context.as_deref())];
    if !detail.releases.is_empty() {
        page.sections.push(typed_section("Releases", detail.releases, None));
    }
    page.state = match detail.state {
        SpotifyContentState::Available => ready_or_empty(page),
        SpotifyContentState::Empty => LoadState::Empty("This collection is empty.".into()),
        SpotifyContentState::Restricted(message) => LoadState::Restricted(message),
        SpotifyContentState::Partial(message) => LoadState::Partial(message),
    };
}

fn detail_line(detail: &SpotifyDetailPage) -> String {
    let mut parts = Vec::new();
    if let Some(owner) = &detail.owner {
        parts.push(format!("by {owner}"));
    }
    if !detail.description.is_empty() {
        parts.push(detail.description.clone());
    }
    if let Some(release) = &detail.release {
        parts.push(release.clone());
    }
    parts.push(format!("{} tracks", detail.total));
    parts.join(" · ")
}

fn ready_or_empty(page: &PageState) -> LoadState {
    if page.sections.iter().all(|section| section.items.is_empty()) {
        LoadState::Empty("Nothing to show.".into())
    } else {
        LoadState::Ready
    }
}

fn display_section(title: &str, items: Vec<SpotifyDisplayItem>) -> Section {
    Section {
        title: title.into(),
        items: items.into_iter().map(|item| display_item(item, None)).collect(),
    }
}

fn display_item(item: SpotifyDisplayItem, fallback: Option<EntityKind>) -> BrowseItem {
    let kind = item.uri.as_deref().and_then(|uri| uri.parse::<SpotifyUri>().ok()).map_or(
        fallback.unwrap_or(EntityKind::Message),
        |uri| match uri.kind() {
            SpotifyItemKind::Track => EntityKind::Track,
            SpotifyItemKind::Album => EntityKind::Album,
            SpotifyItemKind::Artist => EntityKind::Artist,
            SpotifyItemKind::Playlist => EntityKind::Playlist,
        },
    );
    BrowseItem {
        id: item.uri.clone().unwrap_or_else(|| item.title.clone()),
        kind,
        title: item.title,
        subtitle: item.subtitle,
        metadata: String::new(),
        uri: item.uri,
        external_url: None,
        artwork_url: item.artwork_url,
        artists: Vec::new(),
        album: None,
        duration_ms: None,
        available: true,
        context: None,
        restricted: false,
        saved: None,
    }
}

fn typed_section(title: &str, items: Vec<SpotifyBrowseItem>, context: Option<&str>) -> Section {
    Section {
        title: title.into(),
        items: items.into_iter().map(|item| typed_item(item, context)).collect(),
    }
}

fn typed_item(item: SpotifyBrowseItem, context: Option<&str>) -> BrowseItem {
    let position = item.position;
    BrowseItem {
        id: item.id,
        kind: match item.kind {
            SpotifyEntityKind::Track => EntityKind::Track,
            SpotifyEntityKind::Album => EntityKind::Album,
            SpotifyEntityKind::Artist => EntityKind::Artist,
            SpotifyEntityKind::Playlist => EntityKind::Playlist,
        },
        title: item.title,
        subtitle: item.subtitle,
        metadata: item.metadata,
        uri: item.uri,
        external_url: item.external_url,
        artwork_url: item.artwork_url,
        artists: item.artists,
        album: item.album,
        duration_ms: item.duration_ms,
        available: item.available,
        context: context
            .zip(position)
            .map(|(uri, position)| ContextPosition { uri: uri.to_owned(), position }),
        restricted: false,
        saved: item.saved,
    }
}

fn batch_populate_saved_tracks(api: &SpotifyWebApi, token: &str, page: &mut PageState) {
    let mut track_ids = Vec::new();
    for section in &page.sections {
        for item in &section.items {
            if item.kind == EntityKind::Track
                && item.saved.is_none()
                && let Some(uri) = &item.uri
            {
                let id = uri.strip_prefix("spotify:track:").unwrap_or(uri);
                track_ids.push(id.to_string());
            }
        }
    }
    if track_ids.is_empty() {
        return;
    }
    let query_ids = if track_ids.len() > 50 { &track_ids[..50] } else { &track_ids[..] };
    let id_refs: Vec<&str> = query_ids.iter().map(String::as_str).collect();
    if let Ok(saved_flags) = api.check_saved_tracks(token, &id_refs) {
        let mut idx = 0;
        for section in &mut page.sections {
            for item in &mut section.items {
                if item.kind == EntityKind::Track
                    && item.saved.is_none()
                    && item.uri.is_some()
                    && idx < saved_flags.len()
                {
                    item.saved = Some(saved_flags[idx]);
                    idx += 1;
                }
            }
        }
    }
}

fn queue_response(queue: SpotifyTypedQueue) -> ServiceResponse {
    ServiceResponse::Queue {
        now: queue.current.map(|item| typed_item(item, None)),
        upcoming: queue.upcoming.into_iter().map(|item| typed_item(item, None)).collect(),
    }
}

fn setting_item(id: &str, title: &str) -> BrowseItem {
    let mut item = BrowseItem::message(id, title);
    item.kind = EntityKind::Action;
    item.available = true;
    item
}

fn channel_error<T: std::fmt::Display>(error: T) -> AppError {
    AppError::new(ErrorKind::Unavailable, format!("background worker stopped: {error}"))
}

/// Maps a repeat effect numeric value to Spotify's repeat mode string.
#[must_use]
pub const fn repeat_mode(value: u8) -> &'static str {
    match value {
        1 => "context",
        2 => "track",
        _ => "off",
    }
}

fn play_track_with_retry(
    api: &SpotifyWebApi,
    token: &str,
    uri: &str,
    device_id: Option<&str>,
) -> Result<()> {
    let initial = api.play_uri_on_device(token, uri, device_id);
    if let Err(ref error) = initial
        && error.kind == ErrorKind::Unavailable
    {
        let target_device = match device_id {
            Some(id) => Some(id.to_owned()),
            None => resolve_fallback_device(api, token),
        };
        if let Some(ref target) = target_device {
            let _ = api.transfer(token, target);
            thread::sleep(Duration::from_millis(250));
            if api.play_uri_on_device(token, uri, Some(target)).is_ok() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(150));
        if let Ok(playback) = api.playback(token)
            && playback.playing
        {
            return Ok(());
        }
    }
    initial
}

// Fetch every page before replacing playback: a page failure must not silently truncate
// the queue. Positions are the original library positions, including unavailable entries.
fn liked_playback_uris(
    position: usize,
    mut fetch: impl FnMut(u32) -> Result<lspotify_spotify::SpotifyPage<SpotifyBrowseItem>>,
) -> Result<Vec<String>> {
    let mut offset = 0;
    let mut uris = Vec::new();
    let mut selected_available = false;
    loop {
        let page = fetch(offset)?;
        for item in page.items {
            if item.position == Some(position) && item.available && item.uri.is_some() {
                selected_available = true;
            }
            if item.available
                && item.position.is_some_and(|pos| pos >= position)
                && let Some(uri) = item.uri
            {
                uris.push(uri);
            }
        }
        let Some(next) = page.next_offset else { break };
        if next <= offset {
            return Err(AppError::new(
                ErrorKind::Network,
                "Liked Songs pagination did not advance",
            ));
        }
        offset = next;
    }
    if !selected_available {
        return Err(AppError::new(
            ErrorKind::Unavailable,
            "The selected liked song is no longer available; refresh Liked Songs",
        ));
    }
    Ok(uris)
}

fn play_tracks_with_retry(
    api: &SpotifyWebApi,
    token: &str,
    uris: &[String],
    device_id: Option<&str>,
) -> Result<()> {
    let initial = api.play_tracks_on_device(token, uris, device_id);
    if let Err(ref error) = initial
        && error.kind == ErrorKind::Unavailable
    {
        let target_device =
            device_id.map(str::to_owned).or_else(|| resolve_fallback_device(api, token));
        if let Some(ref target) = target_device {
            let _ = api.transfer(token, target);
            thread::sleep(Duration::from_millis(250));
            if api.play_tracks_on_device(token, uris, Some(target)).is_ok() {
                return Ok(());
            }
        }
    }
    initial
}

fn play_context_with_retry(
    api: &SpotifyWebApi,
    token: &str,
    uri: &str,
    position: usize,
    device_id: Option<&str>,
) -> Result<()> {
    if uri == "spotify:collection:tracks" {
        let uris = liked_playback_uris(position, |offset| {
            api.liked_tracks_page(token, offset, PAGE_SIZE)
        })?;
        return play_tracks_with_retry(api, token, &uris, device_id);
    }
    let initial = api.play_context_at(token, uri, position, device_id);
    if let Err(ref error) = initial
        && error.kind == ErrorKind::Unavailable
    {
        let target_device = match device_id {
            Some(id) => Some(id.to_owned()),
            None => resolve_fallback_device(api, token),
        };
        if let Some(ref target) = target_device {
            let _ = api.transfer(token, target);
            thread::sleep(Duration::from_millis(250));
            if api.play_context_at(token, uri, position, Some(target)).is_ok() {
                return Ok(());
            }
        }
        thread::sleep(Duration::from_millis(150));
        if let Ok(playback) = api.playback(token)
            && playback.playing
        {
            return Ok(());
        }
    }
    initial
}

fn transfer_local_when_registered(api: &SpotifyWebApi, token: &str, device_id: &str) -> Result<()> {
    retry_registration(Duration::from_secs(6), |remaining| {
        api.transfer_if_registered(token, device_id, remaining)
    })
}

fn registration_retry_delay(error: &AppError) -> Option<Duration> {
    match error.kind {
        ErrorKind::Unavailable => Some(Duration::from_millis(250)),
        ErrorKind::Network if error.message == "Spotify request timed out" => {
            Some(Duration::from_millis(250))
        }
        ErrorKind::RateLimited => error.retry_after,
        _ => None,
    }
}

fn retry_registration(
    budget: Duration,
    mut attempt: impl FnMut(Duration) -> Result<bool>,
) -> Result<()> {
    let deadline = Instant::now() + budget;
    let mut last_error = AppError::new(
        ErrorKind::Unavailable,
        "local device did not register with Spotify Connect within 6 seconds",
    );
    while let Some(remaining) =
        deadline.checked_duration_since(Instant::now()).filter(|value| !value.is_zero())
    {
        let delay = match attempt(remaining) {
            Ok(true) => return Ok(()),
            Ok(false) => Duration::from_millis(250),
            Err(error) => {
                let Some(delay) = registration_retry_delay(&error) else {
                    return Err(error);
                };
                last_error = error;
                delay
            }
        };
        if delay >= deadline.saturating_duration_since(Instant::now()) {
            break;
        }
        thread::sleep(delay);
    }
    Err(last_error)
}

fn resolve_fallback_device(api: &SpotifyWebApi, token: &str) -> Option<String> {
    let devices = api.devices(token).ok()?;
    devices
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case("lspotify") && !d.restricted)
        .or_else(|| devices.iter().find(|d| d.active && !d.restricted))
        .or_else(|| devices.iter().find(|d| !d.restricted))
        .map(|d| d.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn liked_page(
        offset: u32,
        next_offset: Option<u32>,
    ) -> lspotify_spotify::SpotifyPage<SpotifyBrowseItem> {
        lspotify_spotify::SpotifyPage {
            items: (offset..offset + 50)
                .filter(|pos| *pos != 76)
                .map(|pos| SpotifyBrowseItem {
                    id: pos.to_string(),
                    kind: SpotifyEntityKind::Track,
                    title: pos.to_string(),
                    subtitle: String::new(),
                    metadata: String::new(),
                    // Duplicate songs must remain duplicated and keep their original positions.
                    uri: Some(format!("spotify:track:{}", if pos == 78 { 75 } else { pos })),
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: None,
                    available: pos != 77,
                    position: Some(pos as usize),
                    saved: Some(true),
                })
                .collect(),
            offset,
            limit: 50,
            total: 150,
            next_offset,
        }
    }

    #[test]
    fn liked_playback_loads_all_pages_and_starts_at_original_position() {
        let mut offsets = Vec::new();
        let uris = liked_playback_uris(75, |offset| {
            offsets.push(offset);
            Ok(liked_page(offset, (offset < 100).then_some(offset + 50)))
        })
        .unwrap();
        assert_eq!(offsets, vec![0, 50, 100]);
        assert_eq!(uris.len(), 73);
        assert_eq!(&uris[..3], &["spotify:track:75", "spotify:track:75", "spotify:track:79"]);
        assert_eq!(uris.last().unwrap(), "spotify:track:149");
    }

    #[test]
    fn liked_playback_does_not_play_a_partial_list_on_fetch_failure() {
        let result = liked_playback_uris(0, |offset| {
            if offset == 0 {
                Ok(liked_page(0, Some(50)))
            } else {
                Err(AppError::new(ErrorKind::Network, "page failed"))
            }
        });
        assert_eq!(result.unwrap_err().kind, ErrorKind::Network);
    }

    #[test]
    fn liked_playback_rejects_missing_selection_and_stalled_pagination() {
        assert_eq!(
            liked_playback_uris(76, |offset| Ok(liked_page(offset, None))).unwrap_err().kind,
            ErrorKind::Unavailable
        );
        assert_eq!(
            liked_playback_uris(0, |offset| Ok(liked_page(offset, Some(offset)))).unwrap_err().kind,
            ErrorKind::Network
        );
    }

    #[test]
    fn registration_retries_only_transient_errors_within_budget() {
        for kind in [
            ErrorKind::Authentication,
            ErrorKind::Restricted,
            ErrorKind::Authorization,
            ErrorKind::NotFound,
            ErrorKind::InvalidInput,
            ErrorKind::Network,
        ] {
            let mut calls = 0;
            assert!(
                retry_registration(Duration::from_secs(6), |_| {
                    calls += 1;
                    Err(AppError::new(kind, "final error"))
                })
                .is_err()
            );
            assert_eq!(calls, 1);
        }
        assert!(
            registration_retry_delay(&AppError::new(ErrorKind::Unavailable, "HTTP 503")).is_some()
        );
        assert!(
            registration_retry_delay(&AppError::new(
                ErrorKind::Network,
                "Spotify request timed out"
            ))
            .is_some()
        );
        let mut calls = 0;
        let start = Instant::now();
        assert!(
            retry_registration(Duration::from_millis(10), |remaining| {
                assert!(remaining <= Duration::from_millis(10));
                calls += 1;
                Err(AppError::rate_limited("slow down", Duration::from_secs(60)))
            })
            .is_err()
        );
        assert_eq!(calls, 1);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn poll_generation_is_captured_when_request_is_enqueued() {
        let handle = ServiceHandle::start_with_performer("token".into(), |_, _| {
            ServiceResponse::Playback { playback: SpotifyPlayback::default(), generation: 99 }
        })
        .unwrap();
        handle.send(Effect::RefreshPlayback).unwrap();
        handle.mark_local_mutation();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(ServiceResponse::Playback { generation, .. }) = handle.try_recv() {
                assert_eq!(generation, 0);
                assert_eq!(handle.playback_generation(), 1);
                break;
            }
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
    }

    #[test]
    fn repeat_mode_maps_values_and_out_of_bounds_to_off() {
        assert_eq!(repeat_mode(0), "off");
        assert_eq!(repeat_mode(1), "context");
        assert_eq!(repeat_mode(2), "track");
        assert_eq!(repeat_mode(3), "off");
        assert_eq!(repeat_mode(4), "off");
        assert_eq!(repeat_mode(255), "off");
    }

    #[test]
    fn repeat_effect_pattern_matching_does_not_panic_and_maps_properly() {
        for value in 0..=255 {
            let mode = match value {
                1 => "context",
                2 => "track",
                _ => "off",
            };
            assert_eq!(mode, repeat_mode(value));
            match value {
                1 => assert_eq!(mode, "context"),
                2 => assert_eq!(mode, "track"),
                _ => assert_eq!(mode, "off"),
            }
        }
    }

    #[test]
    fn apply_detail_stores_context_metadata_and_omits_action_section() {
        let mut page = PageState::loading(
            Route::Album { uri: "spotify:album:1".into(), title: "OK Computer".into() },
            1,
        );
        let detail = SpotifyDetailPage {
            title: "OK Computer".into(),
            kind: lspotify_spotify::SpotifyDetailKind::Album,
            uri: "spotify:album:1".into(),
            external_url: Some("https://open.spotify.com/album/1".into()),
            artwork_url: None,
            owner: Some("Radiohead".into()),
            description: String::new(),
            release: Some("1997".into()),
            total: 12,
            items: lspotify_spotify::SpotifyPage {
                items: vec![],
                offset: 0,
                limit: 50,
                total: 0,
                next_offset: None,
            },
            releases: vec![],
            state: SpotifyContentState::Available,
        };
        apply_detail(&mut page, detail);
        assert_eq!(page.uri.as_deref(), Some("spotify:album:1"));
        assert_eq!(page.external_url.as_deref(), Some("https://open.spotify.com/album/1"));
        assert!(!page.sections.iter().any(|s| s.title == "Actions"));
        assert_eq!(page.sections[0].title, "Tracks");
    }

    #[test]
    fn rapid_navigation_skips_obsolete_browse_requests() {
        let home_request = Effect::LoadPage {
            route: Route::Home,
            generation: 0,
            offset: 0,
            query: String::new(),
            tab: None,
        };
        let library_request = Effect::LoadPage {
            route: Route::Library,
            generation: 1,
            offset: 0,
            query: String::new(),
            tab: Some(LibraryTab::LikedSongs),
        };
        let search_request = Effect::LoadPage {
            route: Route::Search,
            generation: 2,
            offset: 0,
            query: "radiohead".into(),
            tab: None,
        };

        assert!(is_obsolete_browse_effect(&home_request, 2));
        assert!(is_obsolete_browse_effect(&library_request, 2));
        assert!(!is_obsolete_browse_effect(&search_request, 2));

        let search_page_2 = Effect::LoadPage {
            route: Route::Search,
            generation: 2,
            offset: 50,
            query: "radiohead".into(),
            tab: None,
        };
        assert!(!is_obsolete_browse_effect(&search_page_2, 2));

        assert!(!is_obsolete_browse_effect(&Effect::TogglePlayback, 2));
        assert!(!is_obsolete_browse_effect(&Effect::RefreshPlayback, 2));
        assert!(!is_obsolete_browse_effect(&Effect::RefreshQueue, 2));
        assert!(!is_obsolete_browse_effect(&Effect::WarmLikedSongs, 2));
    }

    #[test]
    fn already_running_request_rejected_by_state_generation_checks() {
        let mut state = crate::AppState::default();
        state.navigate(Route::Library);
        state.navigate(Route::Search);
        assert_eq!(state.generation, 2);
        assert_eq!(state.page.route, Route::Search);

        let mut late_library_page = PageState::loading(Route::Library, 1);
        late_library_page.state = LoadState::Ready;
        assert!(!state.accept_page(late_library_page));
        assert_eq!(state.page.route, Route::Search);

        let mut search_page = PageState::loading(Route::Search, 2);
        search_page.state = LoadState::Ready;
        assert!(state.accept_page(search_page));
        assert_eq!(state.page.route, Route::Search);
    }

    #[test]
    fn redundant_polls_keep_only_one_request_of_each_kind_pending() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut worker = ServiceHandle::start_with_performer("token".into(), move |_, effect| {
            if effect == Effect::Next {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            }
            ServiceResponse::Command { effect, result: Ok(()) }
        })
        .unwrap();
        worker.send(Effect::Next).unwrap();
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..100 {
            worker.send(Effect::RefreshPlayback).unwrap();
            worker.send(Effect::RefreshQueue).unwrap();
        }
        // Two coalesced polls leave exactly 62 slots for user requests.
        for _ in 0..62 {
            worker.send(Effect::Volume(50)).unwrap();
        }
        assert!(worker.send(Effect::Next).is_err());
        worker.request_shutdown();
        release_tx.send(()).unwrap();
        worker.join_worker();
    }

    #[test]
    fn drop_service_handle_does_not_wait_for_in_flight_request() {
        use std::time::Instant;

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let handle = ServiceHandle::start_with_performer("dummy".into(), move |_token, effect| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            ServiceResponse::Command { effect, result: Ok(()) }
        })
        .unwrap();

        // Queue a priority request that gets picked up by the worker
        handle.send(Effect::RefreshPlayback).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).expect("performer should be running");

        // The request is currently blocked on release_rx (simulating slow network / 20s timeout).
        // Dropping the handle must NOT wait for release_rx / the in-flight request.
        let start = Instant::now();
        drop(handle);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(150),
            "ServiceHandle::drop must return immediately without waiting for worker thread, took {elapsed:?}"
        );

        // Clean up the detached thread
        release_tx.send(()).unwrap();
    }

    #[test]
    fn request_shutdown_does_not_wait_for_in_flight_request() {
        use std::time::Instant;

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let handle = ServiceHandle::start_with_performer("dummy".into(), move |_token, effect| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            ServiceResponse::Command { effect, result: Ok(()) }
        })
        .unwrap();

        handle.send(Effect::RefreshPlayback).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).expect("performer should be running");

        let start = Instant::now();
        handle.request_shutdown();
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(150),
            "ServiceHandle::request_shutdown must return immediately, took {elapsed:?}"
        );
        assert!(handle.is_shutting_down());

        release_tx.send(()).unwrap();
    }

    #[test]
    fn shutdown_succeeds_when_response_channel_is_full_and_consumer_stops_reading() {
        // Channel capacity = 1
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let handle = ServiceHandle::start_with_performer_and_response_capacity(
            "dummy".into(),
            1,
            move |_token, effect| {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                ServiceResponse::Command { effect, result: Ok(()) }
            },
        )
        .unwrap();

        handle.send(Effect::RefreshPlayback).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        // The consumer NEVER reads responses.
        // Request shutdown while the worker is blocked in perform.
        handle.request_shutdown();

        // Release the performer so it attempts to deliver its response to the full channel
        release_tx.send(()).unwrap();

        // The worker must exit cleanly without deadlocking on responses.send()
        let (done_tx, done_rx) = mpsc::channel();
        let mut worker_handle = handle;
        thread::spawn(move || {
            worker_handle.join_worker();
            done_tx.send(()).unwrap();
        });

        assert!(
            done_rx.recv_timeout(Duration::from_millis(500)).is_ok(),
            "Worker must terminate cleanly when consumer is not reading responses"
        );
    }

    #[test]
    fn worker_discards_in_flight_response_after_shutdown_requested() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let mut handle =
            ServiceHandle::start_with_performer("dummy".into(), move |_token, effect| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                ServiceResponse::Command { effect, result: Ok(()) }
            })
            .unwrap();

        handle.send(Effect::RefreshPlayback).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();

        // Request shutdown while request is running
        handle.request_shutdown();

        // Now finish the request
        release_tx.send(()).unwrap();

        // Join worker to verify it exited
        handle.join_worker();

        // Since shutdown was set before request finished, the response must have been discarded
        assert!(handle.try_recv().is_none(), "Response should be discarded when shutting down");
    }

    #[test]
    fn quitting_with_in_flight_page_load_returns_immediately() {
        use std::time::Instant;

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let handle = ServiceHandle::start_with_performer("dummy".into(), move |_token, effect| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            ServiceResponse::Command { effect, result: Ok(()) }
        })
        .unwrap();

        handle
            .send(Effect::LoadPage {
                route: Route::Home,
                generation: 1,
                offset: 0,
                query: String::new(),
                tab: None,
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).expect("page load in flight");

        let start = Instant::now();
        handle.request_shutdown();
        drop(handle);
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "Page load in flight must not delay shutdown"
        );

        release_tx.send(()).unwrap();
    }

    #[test]
    fn quitting_with_in_flight_warm_liked_songs_returns_immediately() {
        use std::time::Instant;

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        let handle = ServiceHandle::start_with_performer("dummy".into(), move |_token, effect| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            ServiceResponse::Command { effect, result: Ok(()) }
        })
        .unwrap();

        handle.send(Effect::WarmLikedSongs).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).expect("warm liked songs in flight");

        let start = Instant::now();
        handle.request_shutdown();
        drop(handle);
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "WarmLikedSongs in flight must not delay shutdown"
        );

        release_tx.send(()).unwrap();
    }

    #[test]
    fn offline_unresponsive_spotify_simulation_does_not_hang_shutdown() {
        use std::time::Instant;

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();

        // Simulate an unresponsive Spotify API call that would block up to 20 seconds
        let handle = ServiceHandle::start_with_performer("dummy".into(), move |_token, effect| {
            started_tx.send(()).unwrap();
            // Sleep / wait on release simulating network hang
            let _ = release_rx.recv_timeout(Duration::from_secs(20));
            ServiceResponse::Command { effect, result: Ok(()) }
        })
        .unwrap();

        handle.send(Effect::RefreshPlayback).unwrap();
        started_rx.recv_timeout(Duration::from_secs(1)).expect("request started");

        let start = Instant::now();
        handle.request_shutdown();
        drop(handle);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_millis(150),
            "Unresponsive Spotify network request must not block exit, took {elapsed:?}"
        );

        release_tx.send(()).unwrap();
    }
}
