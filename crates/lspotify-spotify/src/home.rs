use std::time::Duration;

use lspotify_core::{
    AlbumId, AlbumSummary, AppError, ArtistId, ArtistSummary, BoxFuture, ErrorKind, Image,
    LibraryService, Page, Result, SpotifyItemKind, SpotifyUri, Track, TrackId,
};
use reqwest::{StatusCode, blocking::Client};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::API_BASE_URL;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyDisplayItem {
    pub title: String,
    pub subtitle: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotifyEntityKind {
    Track,
    Album,
    Artist,
    Playlist,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyBrowseItem {
    pub id: String,
    pub kind: SpotifyEntityKind,
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
    pub position: Option<usize>,
    pub saved: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyPage<T> {
    pub items: Vec<T>,
    pub offset: u32,
    pub limit: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpotifyDetailKind {
    Album,
    Playlist,
    Artist,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SpotifyContentState {
    Available,
    Empty,
    Restricted(String),
    Partial(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyDetailPage {
    pub kind: SpotifyDetailKind,
    pub title: String,
    pub description: String,
    pub uri: String,
    pub external_url: Option<String>,
    pub artwork_url: Option<String>,
    pub owner: Option<String>,
    pub release: Option<String>,
    pub total: u32,
    pub items: SpotifyPage<SpotifyBrowseItem>,
    pub releases: Vec<SpotifyBrowseItem>,
    pub state: SpotifyContentState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifySearchItem {
    pub title: String,
    pub subtitle: String,
    pub kind: String,
    pub uri: Option<String>,
    pub artwork_url: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifySection {
    pub items: Vec<SpotifyDisplayItem>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyHome {
    pub recently_played: SpotifySection,
    pub top_artists: SpotifySection,
    pub top_tracks: SpotifySection,
    pub saved_albums: SpotifySection,
    pub playlists: SpotifySection,
}

/// The user's full saved library: every saved album and every playlist, in one payload.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyLibrary {
    pub albums: SpotifySection,
    pub playlists: SpotifySection,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyPlayback {
    pub title: String,
    pub subtitle: String,
    /// The track's artists as `(display name, artist URI)` pairs, in Spotify's order.
    pub artists: Vec<(String, Option<String>)>,
    pub artwork_url: Option<String>,
    pub playing: bool,
    pub progress_ms: u64,
    pub duration_ms: u64,
    pub volume_percent: Option<u8>,
    pub device_name: Option<String>,
    pub track_uri: Option<String>,
    pub context_uri: Option<String>,
    pub album: String,
    pub album_uri: Option<String>,
    pub device_id: Option<String>,
    pub shuffle: bool,
    pub repeat: String,
    pub actions: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpotifyDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub active: bool,
    pub restricted: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyQueue {
    pub current: Option<SpotifyDisplayItem>,
    pub upcoming: Vec<SpotifyDisplayItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyTypedQueue {
    pub current: Option<SpotifyBrowseItem>,
    pub upcoming: Vec<SpotifyBrowseItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpotifyArtistDetail {
    pub name: String,
    pub description: String,
    pub uri: String,
    pub artwork_url: Option<String>,
    pub banner_url: Option<String>,
    pub is_album: bool,
    pub songs: Vec<SpotifyDisplayItem>,
    pub albums: Vec<SpotifyDisplayItem>,
    pub warning: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SpotifyWebApi {
    client: Client,
}

impl SpotifyWebApi {
    /// Builds the authenticated Spotify Web API client.
    ///
    /// # Errors
    ///
    /// Returns an error if the TLS-enabled HTTP client cannot be initialized.
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("lspotify/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(network_error)?;
        Ok(Self { client })
    }

    /// Loads each personalized Home collection independently.
    ///
    /// A restricted endpoint becomes a warning on its section so one capability change does not
    /// blank the entire Home page.
    ///
    /// # Errors
    ///
    /// Returns an error only when the supplied access token is empty.
    pub fn load_home(&self, access_token: &str) -> Result<SpotifyHome> {
        if access_token.is_empty() {
            return Err(AppError::new(ErrorKind::Authentication, "the active session is empty"));
        }
        std::thread::scope(|s| {
            let recently_played = s.spawn(|| section(self.recently_played(access_token)));
            let top_artists = s.spawn(|| section(self.top_artists(access_token)));
            let top_tracks = s.spawn(|| section(self.top_tracks(access_token)));
            let saved_albums = s.spawn(|| section(self.saved_albums(access_token, 4)));
            let playlists = s.spawn(|| section(self.playlists(access_token, 4)));

            Ok(SpotifyHome {
                recently_played: recently_played.join().unwrap_or_default(),
                top_artists: top_artists.join().unwrap_or_default(),
                top_tracks: top_tracks.join().unwrap_or_default(),
                saved_albums: saved_albums.join().unwrap_or_default(),
                playlists: playlists.join().unwrap_or_default(),
            })
        })
    }

    /// Loads the full saved library: saved albums and playlists, each up to 50 items.
    ///
    /// # Errors
    ///
    /// Returns an error only when the supplied access token is empty.
    pub fn load_library(&self, access_token: &str) -> Result<SpotifyLibrary> {
        if access_token.is_empty() {
            return Err(AppError::new(ErrorKind::Authentication, "the active session is empty"));
        }
        std::thread::scope(|s| {
            let albums = s.spawn(|| section(self.saved_albums(access_token, 50)));
            let playlists = s.spawn(|| section(self.playlists(access_token, 50)));

            Ok(SpotifyLibrary {
                albums: albums.join().unwrap_or_default(),
                playlists: playlists.join().unwrap_or_default(),
            })
        })
    }

    /// Sets Spotify Connect shuffle state on the active device.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn shuffle(&self, token: &str, state: bool) -> Result<()> {
        Self::send_empty(
            self.client
                .put(format!("{API_BASE_URL}/me/player/shuffle"))
                .query(&[("state", if state { "true" } else { "false" })]),
            token,
        )
    }

    /// Searches the supported v1 music entity types.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty query, expired authorization, endpoint restrictions, rate
    /// limiting, network failures, or malformed responses.
    pub fn search(&self, access_token: &str, query: &str) -> Result<Vec<SpotifySearchItem>> {
        let query = query.trim();
        if query.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "enter something to search for"));
        }
        let response: SearchResponse = self.get(
            "/search",
            access_token,
            &[("q", query), ("type", "track,album,artist,playlist"), ("limit", "5")],
        )?;
        let mut results = Vec::new();
        results.extend(response.tracks.into_iter().flat_map(|page| {
            page.items.into_iter().map(|track| SpotifySearchItem {
                artwork_url: track.album.as_ref().and_then(|album| first_image(&album.images)),
                uri: track.uri.clone(),
                subtitle: artist_names(&track.artists),
                title: track.name,
                kind: "Track".into(),
            })
        }));
        results.extend(response.albums.into_iter().flat_map(|page| {
            page.items.into_iter().map(|album| SpotifySearchItem {
                artwork_url: first_image(&album.images),
                uri: album.uri.clone(),
                subtitle: artist_names(&album.artists),
                title: album.name,
                kind: "Album".into(),
            })
        }));
        results.extend(response.artists.into_iter().flat_map(|page| {
            page.items.into_iter().map(|artist| SpotifySearchItem {
                artwork_url: first_image(&artist.images),
                uri: artist.uri.clone(),
                subtitle: artist.genres.into_iter().take(2).collect::<Vec<_>>().join(" · "),
                title: artist.name,
                kind: "Artist".into(),
            })
        }));
        results.extend(response.playlists.into_iter().flat_map(|page| {
            page.items.into_iter().flatten().map(|playlist| SpotifySearchItem {
                artwork_url: first_image(&playlist.images),
                uri: playlist.uri.clone(),
                subtitle: playlist.items.map_or_else(
                    || "Spotify playlist".into(),
                    |items| format!("{} items", items.total),
                ),
                title: playlist.name,
                kind: "Playlist".into(),
            })
        }));
        Ok(results)
    }

    /// Searches typed entities with an explicit page offset.
    ///
    /// # Errors
    /// Returns an error for invalid input, authorization, networking, rate limits, or malformed data.
    pub fn search_page(
        &self,
        access_token: &str,
        query: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SpotifyPage<SpotifyBrowseItem>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(SpotifyPage {
                items: Vec::new(),
                offset,
                limit,
                total: 0,
                next_offset: None,
            });
        }
        let response: SearchResponse = self.get(
            "/search",
            access_token,
            &[
                ("q", query),
                ("type", "track,album,artist,playlist"),
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        )?;
        let mut items = Vec::new();
        let mut totals = Vec::new();
        let mut has_next = false;
        if let Some(page) = response.tracks {
            totals.push(page.total);
            has_next |= page.next.is_some();
            items.extend(
                page.items
                    .into_iter()
                    .enumerate()
                    .map(|(position, track)| browse_track(track, Some(position))),
            );
        }
        if let Some(page) = response.albums {
            totals.push(page.total);
            has_next |= page.next.is_some();
            items.extend(page.items.into_iter().map(browse_album));
        }
        if let Some(page) = response.artists {
            totals.push(page.total);
            has_next |= page.next.is_some();
            items.extend(page.items.into_iter().map(browse_artist));
        }
        if let Some(page) = response.playlists {
            totals.push(page.total);
            has_next |= page.next.is_some();
            items.extend(page.items.into_iter().flatten().map(browse_playlist));
        }
        Ok(SpotifyPage {
            items,
            offset,
            limit,
            total: totals.into_iter().sum(),
            next_offset: has_next.then_some(offset.saturating_add(limit)),
        })
    }

    /// Loads one page of the user's liked tracks from `/me/tracks`.
    ///
    /// # Errors
    /// Returns an error when Spotify rejects the request or returns malformed data.
    pub fn liked_tracks_page(
        &self,
        token: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SpotifyPage<SpotifyBrowseItem>> {
        let page: SavedTracksPage = self.get(
            "/me/tracks",
            token,
            &[
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        )?;
        let items = page
            .items
            .into_iter()
            .enumerate()
            .filter_map(|(pos, item)| {
                item.track.map(|track| {
                    let mut b = browse_track(
                        track,
                        Some(usize::try_from(offset).unwrap_or(0).saturating_add(pos)),
                    );
                    b.saved = Some(true);
                    b
                })
            })
            .collect();
        Ok(SpotifyPage {
            items,
            offset: page.offset.unwrap_or(offset),
            limit: page.limit.unwrap_or(limit),
            total: page.total,
            next_offset: page.next.map(|_| offset.saturating_add(limit)),
        })
    }

    /// Loads one page of saved albums without collapsing pagination metadata.
    ///
    /// # Errors
    /// Returns an error when Spotify rejects the request or returns malformed data.
    pub fn saved_albums_page(
        &self,
        token: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SpotifyPage<SpotifyBrowseItem>> {
        let page: AlbumPage = self.get(
            "/me/albums",
            token,
            &[
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        )?;
        Ok(SpotifyPage {
            items: page
                .items
                .into_iter()
                .map(|saved| {
                    let mut item = browse_album(saved.album);
                    item.saved = Some(true);
                    item
                })
                .collect(),
            offset: page.offset.unwrap_or(offset),
            limit: page.limit.unwrap_or(limit),
            total: page.total,
            next_offset: page.next.map(|_| offset.saturating_add(limit)),
        })
    }

    /// Loads one page of the current user's playlists.
    ///
    /// # Errors
    /// Returns an error when Spotify rejects the request or returns malformed data.
    pub fn playlists_page(
        &self,
        token: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SpotifyPage<SpotifyBrowseItem>> {
        let page: PlaylistPage = self.get(
            "/me/playlists",
            token,
            &[
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        )?;
        Ok(SpotifyPage {
            items: page.items.into_iter().map(browse_playlist).collect(),
            offset: page.offset.unwrap_or(offset),
            limit: page.limit.unwrap_or(limit),
            total: page.total,
            next_offset: page.next.map(|_| offset.saturating_add(limit)),
        })
    }

    /// Loads playlist metadata and its `/playlists/{id}/items` page independently.
    /// A Development Mode restriction is represented as content state rather than an empty list.
    ///
    /// # Errors
    /// Returns an error for an invalid URI or when metadata or non-restricted item loading fails.
    pub fn playlist_page(
        &self,
        token: &str,
        uri: &str,
        offset: u32,
        limit: u32,
    ) -> Result<SpotifyDetailPage> {
        let parsed = parse_uri(uri, SpotifyItemKind::Playlist)?;
        let metadata: PlaylistDetailObject =
            self.get(&format!("/playlists/{}", parsed.id()), token, &[])?;
        let items_result = self.get::<PlaylistItemsPage>(
            &format!("/playlists/{}/items", parsed.id()),
            token,
            &[
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        );
        let (items, state) = match items_result {
            Ok(page) => {
                let normalized = normalize_playlist_items(page, offset, limit);
                let state = if normalized.items.is_empty() { SpotifyContentState::Empty } else { SpotifyContentState::Available };
                (normalized, state)
            }
            Err(error) if error.kind == ErrorKind::Restricted => (
                SpotifyPage { items: Vec::new(), offset, limit, total: metadata.items.as_ref().map_or(0, |value| value.total), next_offset: None },
                SpotifyContentState::Restricted("Spotify Development Mode does not expose this playlist's contents. Open it in Spotify to view or play it.".into()),
            ),
            Err(error) => return Err(error),
        };
        Ok(SpotifyDetailPage {
            kind: SpotifyDetailKind::Playlist,
            title: metadata.name,
            description: metadata.description.unwrap_or_default(),
            uri: metadata.uri.unwrap_or_else(|| uri.to_owned()),
            external_url: metadata.external_urls.and_then(|urls| urls.spotify),
            artwork_url: first_image(&metadata.images),
            owner: metadata.owner.and_then(|owner| owner.display_name.or(owner.id)),
            release: None,
            total: items.total,
            items,
            releases: Vec::new(),
            state,
        })
    }

    /// Loads a complete album page with ordered track/disc metadata.
    ///
    /// # Errors
    /// Returns an error for an invalid URI or when Spotify rejects or malforms the response.
    pub fn album_page(&self, token: &str, uri: &str) -> Result<SpotifyDetailPage> {
        let parsed = parse_uri(uri, SpotifyItemKind::Album)?;
        let album: AlbumDetailObject = self.get(&format!("/albums/{}", parsed.id()), token, &[])?;
        let context_uri = album.uri.clone().unwrap_or_else(|| uri.to_owned());
        let cover = first_image(&album.images);
        let mut tracks = album.tracks.unwrap_or_default();
        while tracks.next.is_some() {
            let offset = tracks.offset.unwrap_or_default().saturating_add(
                tracks
                    .limit
                    .unwrap_or_else(|| u32::try_from(tracks.items.len()).unwrap_or(50))
                    .max(1),
            );
            let next: TrackPage = self.get(
                &format!("/albums/{}/tracks", parsed.id()),
                token,
                &[("offset", offset.to_string().as_str()), ("limit", "50")],
            )?;
            tracks.next.clone_from(&next.next);
            tracks.offset = next.offset.or(Some(offset));
            tracks.limit = next.limit.or(Some(50));
            tracks.total = tracks.total.max(next.total);
            tracks.items.extend(next.items);
        }
        let total = tracks.total.max(u32::try_from(tracks.items.len()).unwrap_or(u32::MAX));
        let items = tracks
            .items
            .into_iter()
            .enumerate()
            .map(|(position, track)| {
                let disc = track.disc_number.unwrap_or(1);
                let number = track
                    .track_number
                    .unwrap_or_else(|| u32::try_from(position + 1).unwrap_or(u32::MAX));
                let mut item = browse_track(track, Some(position));
                item.artwork_url.clone_from(&cover);
                item.metadata = format!("Disc {disc} · Track {number}");
                item
            })
            .collect();
        let description = artist_names(&album.artists);
        Ok(SpotifyDetailPage {
            kind: SpotifyDetailKind::Album,
            title: album.name,
            description,
            uri: context_uri,
            external_url: album.external_urls.and_then(|urls| urls.spotify),
            artwork_url: cover,
            owner: None,
            release: album.release_date,
            total,
            items: SpotifyPage { items, offset: 0, limit: total, total, next_offset: None },
            releases: Vec::new(),
            state: if total == 0 {
                SpotifyContentState::Empty
            } else {
                SpotifyContentState::Available
            },
        })
    }

    /// Loads an artist plus search-derived songs and releases.
    ///
    /// # Errors
    /// Returns an error for an invalid URI or when the artist profile cannot be loaded.
    pub fn artist_page(&self, token: &str, uri: &str) -> Result<SpotifyDetailPage> {
        let detail = self.artist(token, uri)?;
        let songs = detail
            .songs
            .into_iter()
            .enumerate()
            .map(|(position, item)| browse_display(item, SpotifyEntityKind::Track, Some(position)))
            .collect::<Vec<_>>();
        let releases = detail
            .albums
            .into_iter()
            .map(|item| browse_display(item, SpotifyEntityKind::Album, None))
            .collect::<Vec<_>>();
        let state =
            detail.warning.map_or(SpotifyContentState::Available, SpotifyContentState::Partial);
        Ok(SpotifyDetailPage {
            kind: SpotifyDetailKind::Artist,
            title: detail.name,
            description: detail.description,
            uri: detail.uri,
            external_url: None,
            artwork_url: detail.artwork_url,
            owner: None,
            release: None,
            total: u32::try_from(songs.len()).unwrap_or(u32::MAX),
            items: SpotifyPage {
                total: u32::try_from(songs.len()).unwrap_or(u32::MAX),
                items: songs,
                offset: 0,
                limit: 10,
                next_offset: None,
            },
            releases,
            state,
        })
    }

    /// Loads an artist profile, releases, and supported song search results.
    ///
    /// Spotify removed the dedicated artist-top-tracks endpoint from Development Mode in 2026,
    /// so songs are sourced from catalog search relevance instead.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid artist URI or when the artist profile cannot be loaded.
    pub fn artist(&self, access_token: &str, uri: &str) -> Result<SpotifyArtistDetail> {
        let parsed = uri
            .parse::<SpotifyUri>()
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid Spotify artist URI"))?;
        if parsed.kind() != SpotifyItemKind::Artist {
            return Err(AppError::new(ErrorKind::InvalidInput, "expected a Spotify artist URI"));
        }
        let artist: ArtistObject =
            self.get(&format!("/artists/{}", parsed.id()), access_token, &[])?;
        let albums = self.get::<SearchAlbumPage>(
            &format!("/artists/{}/albums", parsed.id()),
            access_token,
            &[("include_groups", "album,single"), ("limit", "10")],
        );
        let query = format!("artist:\"{}\"", artist.name.replace('"', ""));
        let songs = self.get::<SearchResponse>(
            "/search",
            access_token,
            &[("q", query.as_str()), ("type", "track"), ("limit", "10")],
        );
        let mut warnings = Vec::new();
        let albums = match albums {
            Ok(page) => page
                .items
                .into_iter()
                .map(|album| SpotifyDisplayItem {
                    artwork_url: first_image(&album.images),
                    uri: album.uri.clone(),
                    subtitle: artist_names(&album.artists),
                    title: album.name,
                })
                .collect(),
            Err(error) => {
                warnings.push(error.to_string());
                Vec::new()
            }
        };
        let songs = match songs {
            Ok(response) => response
                .tracks
                .into_iter()
                .flat_map(|page| page.items.into_iter().map(track_item))
                .collect(),
            Err(error) => {
                warnings.push(error.to_string());
                Vec::new()
            }
        };
        let artwork_url = first_image(&artist.images);
        Ok(SpotifyArtistDetail {
            description: if artist.genres.is_empty() {
                "Artist on Spotify".into()
            } else {
                artist.genres.join(" · ")
            },
            uri: artist.uri.unwrap_or_else(|| uri.to_owned()),
            banner_url: artwork_url.clone(),
            artwork_url,
            is_album: false,
            name: artist.name,
            songs,
            albums,
            warning: (!warnings.is_empty()).then(|| warnings.join(" · ")),
        })
    }

    /// Loads an album page: its cover, artists, and full track list.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid album URI or when the album cannot be loaded.
    pub fn album(&self, access_token: &str, uri: &str) -> Result<SpotifyArtistDetail> {
        let parsed = uri
            .parse::<SpotifyUri>()
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid Spotify album URI"))?;
        if parsed.kind() != SpotifyItemKind::Album {
            return Err(AppError::new(ErrorKind::InvalidInput, "expected a Spotify album URI"));
        }
        let album: AlbumDetailObject =
            self.get(&format!("/albums/{}", parsed.id()), access_token, &[("limit", "50")])?;
        let cover = first_image(&album.images);
        // Album track objects omit their own artwork, so reuse the album cover for every row.
        let songs = album
            .tracks
            .map(|page| page.items)
            .unwrap_or_default()
            .into_iter()
            .map(|track| SpotifyDisplayItem {
                artwork_url: cover.clone(),
                uri: track.uri,
                title: track.name,
                subtitle: artist_names(&track.artists),
            })
            .collect();
        let description = {
            let artists = artist_names(&album.artists);
            match album.release_date.as_deref().and_then(|date| date.get(0..4)) {
                Some(year) if !artists.is_empty() => format!("{artists} · {year}"),
                Some(year) => year.to_owned(),
                None => artists,
            }
        };
        Ok(SpotifyArtistDetail {
            description,
            uri: album.uri.unwrap_or_else(|| uri.to_owned()),
            banner_url: cover.clone(),
            artwork_url: cover,
            is_album: true,
            name: album.name,
            songs,
            albums: Vec::new(),
            warning: None,
        })
    }

    /// Gets the current Spotify Connect playback state.
    ///
    /// # Errors
    ///
    /// Returns an error for expired authorization, restrictions, rate limits, network failures, or
    /// malformed responses.
    pub fn playback(&self, token: &str) -> Result<SpotifyPlayback> {
        let response = self
            .client
            .get(format!("{API_BASE_URL}/me/player"))
            .bearer_auth(token)
            .send()
            .map_err(network_error)?;
        if response.status() == StatusCode::NO_CONTENT {
            return Ok(SpotifyPlayback::default());
        }
        let status = response.status();
        if !status.is_success() {
            return Err(response_error(&response));
        }
        let state: PlaybackResponse = response.json().map_err(|error| invalid_data(&error))?;
        let item = state.item;
        let album = item.as_ref().and_then(|track| track.album.as_ref());
        Ok(SpotifyPlayback {
            title: item.as_ref().map_or_else(String::new, |track| track.name.clone()),
            subtitle: item.as_ref().map_or_else(String::new, |track| artist_names(&track.artists)),
            artists: item.as_ref().map_or_else(Vec::new, |track| {
                track
                    .artists
                    .iter()
                    .map(|artist| (artist.name.clone(), artist.uri.clone()))
                    .collect()
            }),
            artwork_url: item
                .as_ref()
                .and_then(|track| track.album.as_ref())
                .and_then(|album| first_image(&album.images)),
            playing: state.is_playing,
            progress_ms: state.progress_ms.unwrap_or_default(),
            duration_ms: item.as_ref().and_then(|track| track.duration_ms).unwrap_or_default(),
            volume_percent: state.device.as_ref().and_then(|device| device.volume_percent),
            device_name: state.device.as_ref().map(|device| device.name.clone()),
            track_uri: item.as_ref().and_then(|track| track.uri.clone()),
            context_uri: state.context.and_then(|context| context.uri),
            album: album.map_or_else(String::new, |album| album.name.clone()),
            album_uri: album.and_then(|album| album.uri.clone()),
            device_id: state.device.as_ref().and_then(|device| device.id.clone()),
            shuffle: state.shuffle_state,
            repeat: state.repeat_state.unwrap_or_else(|| "off".into()),
            actions: state.actions.map_or_else(Vec::new, PlaybackActions::available),
        })
    }

    /// Gets available Spotify Connect devices.
    ///
    /// # Errors
    ///
    /// Returns an error for authorization, restrictions, rate limits, networking, or invalid data.
    pub fn devices(&self, token: &str) -> Result<Vec<SpotifyDevice>> {
        let response: DevicesResponse = self.get("/me/player/devices", token, &[])?;
        Ok(response
            .devices
            .into_iter()
            .filter_map(|device| {
                device.id.map(|id| SpotifyDevice {
                    id,
                    name: device.name,
                    kind: device.kind,
                    active: device.is_active,
                    restricted: device.is_restricted,
                })
            })
            .collect())
    }

    /// Gets the readable Spotify queue.
    ///
    /// # Errors
    ///
    /// Returns an error for authorization, restrictions, rate limits, networking, or invalid data.
    pub fn queue(&self, token: &str) -> Result<SpotifyQueue> {
        let response: QueueResponse = self.get("/me/player/queue", token, &[])?;
        Ok(SpotifyQueue {
            current: response.currently_playing.map(track_item),
            upcoming: response.queue.into_iter().map(track_item).collect(),
        })
    }

    /// Gets the queue with typed identity, album, artwork, and duration metadata.
    ///
    /// # Errors
    /// Returns an error for authorization, restrictions, rate limits, networking, or invalid data.
    pub fn typed_queue(&self, token: &str) -> Result<SpotifyTypedQueue> {
        let response: QueueResponse = self.get("/me/player/queue", token, &[])?;
        Ok(SpotifyTypedQueue {
            current: response.currently_playing.map(|track| browse_track(track, None)),
            upcoming: response
                .queue
                .into_iter()
                .enumerate()
                .map(|(position, track)| browse_track(track, Some(position)))
                .collect(),
        })
    }

    /// Starts a track or context URI on the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when the URI is unsupported or Spotify rejects the command.
    pub fn play_uri(&self, token: &str, uri: &str) -> Result<()> {
        self.play_uri_on_device(token, uri, None)
    }

    /// Starts a URI on a specific Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when the URI is unsupported or Spotify rejects the command.
    pub fn play_uri_on_device(
        &self,
        token: &str,
        uri: &str,
        device_id: Option<&str>,
    ) -> Result<()> {
        let body = if uri.starts_with("spotify:track:") {
            PlayBody { uris: Some(vec![uri]), context_uri: None, offset: None }
        } else if uri.starts_with("spotify:album:")
            || uri.starts_with("spotify:artist:")
            || uri.starts_with("spotify:playlist:")
        {
            PlayBody { uris: None, context_uri: Some(uri), offset: None }
        } else {
            return Err(AppError::new(ErrorKind::InvalidInput, "unsupported Spotify play URI"));
        };
        let mut request = self.client.put(format!("{API_BASE_URL}/me/player/play"));
        if let Some(device_id) = device_id {
            request = request.query(&[("device_id", device_id)]);
        }
        Self::send_command(request, token, &body)
    }

    /// Starts a context at its original zero-based position. Positional offsets preserve duplicate
    /// tracks, unlike URI offsets which always identify the first matching track.
    ///
    /// # Errors
    /// Returns an error for an invalid context, an excessive position, or a rejected command.
    pub fn play_context_at(
        &self,
        token: &str,
        context_uri: &str,
        position: usize,
        device_id: Option<&str>,
    ) -> Result<()> {
        let parsed = context_uri
            .parse::<SpotifyUri>()
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid Spotify context URI"))?;
        if !matches!(parsed.kind(), SpotifyItemKind::Album | SpotifyItemKind::Playlist) {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "context offsets require an album or playlist",
            ));
        }
        let position = u32::try_from(position)
            .map_err(|_| AppError::new(ErrorKind::InvalidInput, "context position is too large"))?;
        let body = PlayBody {
            uris: None,
            context_uri: Some(context_uri),
            offset: Some(PlayOffset { position }),
        };
        let mut request = self.client.put(format!("{API_BASE_URL}/me/player/play"));
        if let Some(device_id) = device_id {
            request = request.query(&[("device_id", device_id)]);
        }
        Self::send_command(request, token, &body)
    }

    /// Resumes the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn resume(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.put(format!("{API_BASE_URL}/me/player/play")), token)
    }

    /// Pauses the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn pause(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.put(format!("{API_BASE_URL}/me/player/pause")), token)
    }

    /// Skips to the next queue item.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn next(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.post(format!("{API_BASE_URL}/me/player/next")), token)
    }

    /// Skips to the previous queue item.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn previous(&self, token: &str) -> Result<()> {
        Self::send_empty(self.client.post(format!("{API_BASE_URL}/me/player/previous")), token)
    }

    /// Seeks the active item.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn seek(&self, token: &str, position_ms: u64) -> Result<()> {
        Self::send_empty(
            self.client
                .put(format!("{API_BASE_URL}/me/player/seek"))
                .query(&[("position_ms", position_ms)]),
            token,
        )
    }

    /// Sets active-device volume.
    ///
    /// # Errors
    ///
    /// Returns an error when Spotify rejects the command.
    pub fn volume(&self, token: &str, percent: u8) -> Result<()> {
        Self::send_empty(
            self.client
                .put(format!("{API_BASE_URL}/me/player/volume"))
                .query(&[("volume_percent", percent.min(100))]),
            token,
        )
    }

    /// Sets the repeat mode for the active Spotify Connect device.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsupported mode or when Spotify rejects the command.
    pub fn repeat(&self, token: &str, mode: &str) -> Result<()> {
        if !matches!(mode, "off" | "context" | "track") {
            return Err(AppError::new(ErrorKind::InvalidInput, "unsupported repeat mode"));
        }
        Self::send_empty(
            self.client.put(format!("{API_BASE_URL}/me/player/repeat")).query(&[("state", mode)]),
            token,
        )
    }

    /// Adds one validated track to the active Spotify Connect queue.
    ///
    /// # Errors
    ///
    /// Returns an error when `uri` is not a Spotify track URI or Spotify rejects the command.
    pub fn enqueue(&self, token: &str, uri: &str) -> Result<()> {
        let parsed = uri.parse::<SpotifyUri>().map_err(|_| {
            AppError::new(ErrorKind::InvalidInput, "invalid Spotify URI for queue insertion")
        })?;
        if parsed.kind() != SpotifyItemKind::Track {
            return Err(AppError::new(
                ErrorKind::InvalidInput,
                "only tracks can be added to queue",
            ));
        }
        Self::send_empty(
            self.client.post(format!("{API_BASE_URL}/me/player/queue")).query(&[("uri", uri)]),
            token,
        )
    }

    /// Transfers Spotify playback to one selected device.
    ///
    /// # Errors
    ///
    /// Returns an error when the identifier is empty or Spotify rejects the command.
    pub fn transfer(&self, token: &str, device_id: &str) -> Result<()> {
        if device_id.is_empty() {
            return Err(AppError::new(ErrorKind::InvalidInput, "device identifier is empty"));
        }
        Self::send_command(
            self.client.put(format!("{API_BASE_URL}/me/player")),
            token,
            &TransferBody { device_ids: [device_id], play: false },
        )
    }

    /// Checks registration and transfers within the supplied total time budget.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects a request or the time budget expires.
    pub fn transfer_if_registered(
        &self,
        token: &str,
        device_id: &str,
        budget: Duration,
    ) -> Result<bool> {
        let deadline = std::time::Instant::now() + budget;
        let response = self
            .client
            .get(format!("{API_BASE_URL}/me/player/devices"))
            .bearer_auth(token)
            .timeout(budget)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(registration_error(&response));
        }
        let devices: DevicesResponse = response.json().map_err(|error| invalid_data(&error))?;
        if !devices
            .devices
            .iter()
            .any(|device| device.id.as_deref() == Some(device_id) && !device.is_restricted)
        {
            return Ok(false);
        }
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| AppError::new(ErrorKind::Network, "Spotify request timed out"))?;
        let response = self
            .client
            .put(format!("{API_BASE_URL}/me/player"))
            .bearer_auth(token)
            .timeout(remaining)
            .json(&TransferBody { device_ids: [device_id], play: false })
            .send()
            .map_err(network_error)?;
        if response.status().is_success() { Ok(true) } else { Err(registration_error(&response)) }
    }

    /// Checks if tracks are in the current user's library.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request or returns malformed data.
    pub fn check_saved_tracks(&self, token: &str, track_ids: &[&str]) -> Result<Vec<bool>> {
        if track_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(track_ids.len());
        for chunk in track_ids.chunks(50) {
            let ids_str = chunk.join(",");
            let chunk_res: Vec<bool> =
                self.get("/me/tracks/contains", token, &[("ids", &ids_str)])?;
            results.extend(chunk_res);
        }
        Ok(results)
    }

    /// Saves tracks to the current user's library.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn save_tracks(&self, token: &str, track_ids: &[&str]) -> Result<()> {
        if track_ids.is_empty() {
            return Ok(());
        }
        for chunk in track_ids.chunks(50) {
            let body = IdsBody { ids: chunk };
            let request = self.client.put(format!("{API_BASE_URL}/me/tracks"));
            Self::send_command(request, token, &body)?;
        }
        Ok(())
    }

    /// Removes tracks from the current user's library.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn remove_tracks(&self, token: &str, track_ids: &[&str]) -> Result<()> {
        if track_ids.is_empty() {
            return Ok(());
        }
        for chunk in track_ids.chunks(50) {
            let body = IdsBody { ids: chunk };
            let request = self.client.delete(format!("{API_BASE_URL}/me/tracks"));
            Self::send_command(request, token, &body)?;
        }
        Ok(())
    }

    /// Checks if albums are in the current user's library.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request or returns malformed data.
    pub fn check_saved_albums(&self, token: &str, album_ids: &[&str]) -> Result<Vec<bool>> {
        if album_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(album_ids.len());
        for chunk in album_ids.chunks(50) {
            let ids_str = chunk.join(",");
            let chunk_res: Vec<bool> =
                self.get("/me/albums/contains", token, &[("ids", &ids_str)])?;
            results.extend(chunk_res);
        }
        Ok(results)
    }

    /// Saves albums to the current user's library.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn save_albums(&self, token: &str, album_ids: &[&str]) -> Result<()> {
        if album_ids.is_empty() {
            return Ok(());
        }
        for chunk in album_ids.chunks(50) {
            let body = IdsBody { ids: chunk };
            let request = self.client.put(format!("{API_BASE_URL}/me/albums"));
            Self::send_command(request, token, &body)?;
        }
        Ok(())
    }

    /// Removes albums from the current user's library.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn remove_albums(&self, token: &str, album_ids: &[&str]) -> Result<()> {
        if album_ids.is_empty() {
            return Ok(());
        }
        for chunk in album_ids.chunks(50) {
            let body = IdsBody { ids: chunk };
            let request = self.client.delete(format!("{API_BASE_URL}/me/albums"));
            Self::send_command(request, token, &body)?;
        }
        Ok(())
    }

    /// Follows artists on Spotify.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn follow_artists(&self, token: &str, artist_ids: &[&str]) -> Result<()> {
        if artist_ids.is_empty() {
            return Ok(());
        }
        let ids_str = artist_ids.join(",");
        let request = self
            .client
            .put(format!("{API_BASE_URL}/me/following"))
            .query(&[("type", "artist"), ("ids", &ids_str)]);
        Self::send_empty(request, token)
    }

    /// Unfollows artists on Spotify.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn unfollow_artists(&self, token: &str, artist_ids: &[&str]) -> Result<()> {
        if artist_ids.is_empty() {
            return Ok(());
        }
        let ids_str = artist_ids.join(",");
        let request = self
            .client
            .delete(format!("{API_BASE_URL}/me/following"))
            .query(&[("type", "artist"), ("ids", &ids_str)]);
        Self::send_empty(request, token)
    }

    /// Checks if the user follows artists on Spotify.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn check_following_artists(&self, token: &str, artist_ids: &[&str]) -> Result<Vec<bool>> {
        if artist_ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut results = Vec::with_capacity(artist_ids.len());
        for chunk in artist_ids.chunks(50) {
            let ids_str = chunk.join(",");
            let chunk_res: Vec<bool> = self.get(
                "/me/following/contains",
                token,
                &[("type", "artist"), ("ids", &ids_str)],
            )?;
            results.extend(chunk_res);
        }
        Ok(results)
    }

    /// Saves library items (tracks, albums, or artists) dispatching by URI kind.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the mutation.
    pub fn save_library_items(&self, token: &str, uris: &[&str]) -> Result<()> {
        let mut tracks = Vec::new();
        let mut albums = Vec::new();
        let mut artists = Vec::new();
        for uri in uris {
            if let Ok(parsed) = uri.parse::<SpotifyUri>() {
                match parsed.kind() {
                    SpotifyItemKind::Track => tracks.push(parsed.id().to_owned()),
                    SpotifyItemKind::Album => albums.push(parsed.id().to_owned()),
                    SpotifyItemKind::Artist => artists.push(parsed.id().to_owned()),
                    SpotifyItemKind::Playlist => {}
                }
            }
        }
        if !tracks.is_empty() {
            let refs: Vec<&str> = tracks.iter().map(String::as_str).collect();
            self.save_tracks(token, &refs)?;
        }
        if !albums.is_empty() {
            let refs: Vec<&str> = albums.iter().map(String::as_str).collect();
            self.save_albums(token, &refs)?;
        }
        if !artists.is_empty() {
            let refs: Vec<&str> = artists.iter().map(String::as_str).collect();
            self.follow_artists(token, &refs)?;
        }
        Ok(())
    }

    /// Removes library items (tracks, albums, or artists) dispatching by URI kind.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the mutation.
    pub fn remove_library_items(&self, token: &str, uris: &[&str]) -> Result<()> {
        let mut tracks = Vec::new();
        let mut albums = Vec::new();
        let mut artists = Vec::new();
        for uri in uris {
            if let Ok(parsed) = uri.parse::<SpotifyUri>() {
                match parsed.kind() {
                    SpotifyItemKind::Track => tracks.push(parsed.id().to_owned()),
                    SpotifyItemKind::Album => albums.push(parsed.id().to_owned()),
                    SpotifyItemKind::Artist => artists.push(parsed.id().to_owned()),
                    SpotifyItemKind::Playlist => {}
                }
            }
        }
        if !tracks.is_empty() {
            let refs: Vec<&str> = tracks.iter().map(String::as_str).collect();
            self.remove_tracks(token, &refs)?;
        }
        if !albums.is_empty() {
            let refs: Vec<&str> = albums.iter().map(String::as_str).collect();
            self.remove_albums(token, &refs)?;
        }
        if !artists.is_empty() {
            let refs: Vec<&str> = artists.iter().map(String::as_str).collect();
            self.unfollow_artists(token, &refs)?;
        }
        Ok(())
    }

    /// Checks if library items are saved, maintaining URI order.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn contains_library_items(&self, token: &str, uris: &[&str]) -> Result<Vec<bool>> {
        let mut results = Vec::with_capacity(uris.len());
        for uri in uris {
            if let Ok(parsed) = uri.parse::<SpotifyUri>() {
                let saved = match parsed.kind() {
                    SpotifyItemKind::Track => self
                        .check_saved_tracks(token, &[parsed.id()])?
                        .into_iter()
                        .next()
                        .unwrap_or(false),
                    SpotifyItemKind::Album => self
                        .check_saved_albums(token, &[parsed.id()])?
                        .into_iter()
                        .next()
                        .unwrap_or(false),
                    SpotifyItemKind::Artist => self
                        .check_following_artists(token, &[parsed.id()])?
                        .into_iter()
                        .next()
                        .unwrap_or(false),
                    SpotifyItemKind::Playlist => false,
                };
                results.push(saved);
            } else {
                results.push(false);
            }
        }
        Ok(results)
    }

    /// Adds tracks to a playlist.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn add_to_playlist(
        &self,
        token: &str,
        playlist_id: &str,
        track_uris: &[&str],
    ) -> Result<String> {
        let clean_id = playlist_id.rsplit(':').next().unwrap_or(playlist_id);
        let body = AddPlaylistTracksBody { uris: track_uris };
        let response = self
            .client
            .post(format!("{API_BASE_URL}/playlists/{clean_id}/tracks"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(response_error(&response));
        }
        let snap: SnapshotResponse =
            response.json().unwrap_or(SnapshotResponse { snapshot_id: None });
        Ok(snap.snapshot_id.unwrap_or_default())
    }

    /// Removes tracks from a playlist.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn remove_from_playlist(
        &self,
        token: &str,
        playlist_id: &str,
        track_uris: &[&str],
    ) -> Result<String> {
        let clean_id = playlist_id.rsplit(':').next().unwrap_or(playlist_id);
        let body = RemovePlaylistTracksBody {
            tracks: track_uris.iter().map(|u| TrackUriObject { uri: u }).collect(),
        };
        let response = self
            .client
            .delete(format!("{API_BASE_URL}/playlists/{clean_id}/tracks"))
            .bearer_auth(token)
            .json(&body)
            .send()
            .map_err(network_error)?;
        if !response.status().is_success() {
            return Err(response_error(&response));
        }
        let snap: SnapshotResponse =
            response.json().unwrap_or(SnapshotResponse { snapshot_id: None });
        Ok(snap.snapshot_id.unwrap_or_default())
    }

    /// Renames a playlist.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn update_playlist(&self, token: &str, playlist_id: &str, name: &str) -> Result<()> {
        let clean_id = playlist_id.rsplit(':').next().unwrap_or(playlist_id);
        let body = UpdatePlaylistBody { name };
        let request = self.client.put(format!("{API_BASE_URL}/playlists/{clean_id}"));
        Self::send_command(request, token, &body)
    }

    /// Loads liked tracks conforming to `lspotify_core::Page<Track>`.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn liked_tracks_core(
        &self,
        token: &str,
        offset: u32,
        limit: u32,
    ) -> Result<lspotify_core::Page<lspotify_core::Track>> {
        let page: SavedTracksPage = self.get(
            "/me/tracks",
            token,
            &[
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        )?;
        let items: Vec<lspotify_core::Track> = page
            .items
            .into_iter()
            .filter_map(|item| item.track.and_then(|t| track_object_to_core(t).ok()))
            .collect();
        Ok(lspotify_core::Page {
            items,
            offset: page.offset.unwrap_or(offset),
            limit: page.limit.unwrap_or(limit),
            total: page.total,
            next: page.next,
        })
    }

    /// Loads saved albums conforming to `lspotify_core::Page<AlbumSummary>`.
    ///
    /// # Errors
    /// Returns an error if Spotify rejects the request.
    pub fn saved_albums_core(
        &self,
        token: &str,
        offset: u32,
        limit: u32,
    ) -> Result<lspotify_core::Page<lspotify_core::AlbumSummary>> {
        let page: AlbumPage = self.get(
            "/me/albums",
            token,
            &[
                ("offset", offset.to_string().as_str()),
                ("limit", limit.min(50).to_string().as_str()),
            ],
        )?;
        let items: Vec<lspotify_core::AlbumSummary> = page
            .items
            .into_iter()
            .filter_map(|item| album_object_to_summary(item.album).ok())
            .collect();
        Ok(lspotify_core::Page {
            items,
            offset: page.offset.unwrap_or(offset),
            limit: page.limit.unwrap_or(limit),
            total: page.total,
            next: page.next,
        })
    }

    fn recently_played(&self, token: &str) -> Result<Vec<SpotifyDisplayItem>> {
        let page: HistoryPage = self.get("/me/player/recently-played", token, &[("limit", "4")])?;
        Ok(page.items.into_iter().map(|item| track_item(item.track)).collect())
    }

    fn top_artists(&self, token: &str) -> Result<Vec<SpotifyDisplayItem>> {
        let page: ArtistPage =
            self.get("/me/top/artists", token, &[("limit", "4"), ("time_range", "medium_term")])?;
        Ok(page
            .items
            .into_iter()
            .map(|artist| SpotifyDisplayItem {
                artwork_url: first_image(&artist.images),
                uri: artist.uri.clone(),
                subtitle: artist.genres.into_iter().take(2).collect::<Vec<_>>().join(" · "),
                title: artist.name,
            })
            .collect())
    }

    fn top_tracks(&self, token: &str) -> Result<Vec<SpotifyDisplayItem>> {
        let page: TrackPage =
            self.get("/me/top/tracks", token, &[("limit", "4"), ("time_range", "medium_term")])?;
        Ok(page.items.into_iter().map(track_item).collect())
    }

    fn saved_albums(&self, token: &str, limit: u32) -> Result<Vec<SpotifyDisplayItem>> {
        let page: AlbumPage =
            self.get("/me/albums", token, &[("limit", limit.to_string().as_str())])?;
        Ok(page
            .items
            .into_iter()
            .map(|item| SpotifyDisplayItem {
                artwork_url: first_image(&item.album.images),
                uri: item.album.uri.clone(),
                subtitle: artist_names(&item.album.artists),
                title: item.album.name,
            })
            .collect())
    }

    fn playlists(&self, token: &str, limit: u32) -> Result<Vec<SpotifyDisplayItem>> {
        let page: PlaylistPage =
            self.get("/me/playlists", token, &[("limit", limit.to_string().as_str())])?;
        Ok(page
            .items
            .into_iter()
            .map(|playlist| SpotifyDisplayItem {
                artwork_url: first_image(&playlist.images),
                uri: playlist.uri.clone(),
                subtitle: playlist
                    .items
                    .map_or_else(|| "Playlist".into(), |items| format!("{} items", items.total)),
                title: playlist.name,
            })
            .collect())
    }

    fn send_empty(request: reqwest::blocking::RequestBuilder, token: &str) -> Result<()> {
        // Spotify's front end rejects body-less PUT/POST requests with HTTP 411 unless the
        // request states an explicit zero length.
        let response = request
            .bearer_auth(token)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .body(Vec::new())
            .send()
            .map_err(network_error)?;
        if response.status().is_success() { Ok(()) } else { Err(response_error(&response)) }
    }

    fn send_command(
        request: reqwest::blocking::RequestBuilder,
        token: &str,
        body: &impl Serialize,
    ) -> Result<()> {
        let response = request.bearer_auth(token).json(body).send().map_err(network_error)?;
        if response.status().is_success() { Ok(()) } else { Err(response_error(&response)) }
    }

    fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
        query: &[(&str, &str)],
    ) -> Result<T> {
        let response = self
            .client
            .get(format!("{API_BASE_URL}{path}"))
            .bearer_auth(token)
            .query(query)
            .send()
            .map_err(network_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(response_error(&response));
        }
        response.json().map_err(|error| {
            AppError::new(ErrorKind::Network, format!("Spotify returned invalid data: {error}"))
        })
    }
}

fn section(result: Result<Vec<SpotifyDisplayItem>>) -> SpotifySection {
    match result {
        Ok(items) => SpotifySection { items, warning: None },
        Err(error) => SpotifySection { items: Vec::new(), warning: Some(error.to_string()) },
    }
}

fn track_item(track: TrackObject) -> SpotifyDisplayItem {
    SpotifyDisplayItem {
        artwork_url: track.album.as_ref().and_then(|album| first_image(&album.images)),
        uri: track.uri,
        title: track.name,
        subtitle: artist_names(&track.artists),
    }
}

fn parse_uri(uri: &str, expected: SpotifyItemKind) -> Result<SpotifyUri> {
    let parsed = uri
        .parse::<SpotifyUri>()
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid Spotify URI"))?;
    if parsed.kind() != expected {
        return Err(AppError::new(ErrorKind::InvalidInput, "unexpected Spotify entity type"));
    }
    Ok(parsed)
}

fn id_from_uri(uri: Option<&str>, fallback: &str) -> String {
    uri.and_then(|value| value.rsplit(':').next())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn browse_track(track: TrackObject, position: Option<usize>) -> SpotifyBrowseItem {
    let album = track.album.as_ref();
    let uri = track.uri.clone();
    let supported = uri
        .as_deref()
        .and_then(|value| value.parse::<SpotifyUri>().ok())
        .is_some_and(|value| value.kind() == SpotifyItemKind::Track);
    SpotifyBrowseItem {
        id: id_from_uri(uri.as_deref(), &format!("track-{}", position.unwrap_or_default())),
        kind: SpotifyEntityKind::Track,
        title: track.name,
        subtitle: artist_names(&track.artists),
        metadata: format!(
            "Disc {} · Track {}{}",
            track.disc_number.unwrap_or(1),
            track.track_number.unwrap_or_else(|| position
                .and_then(|value| u32::try_from(value + 1).ok())
                .unwrap_or(1)),
            if track.is_local { " · local" } else { "" }
        ),
        uri,
        external_url: track.external_urls.and_then(|urls| urls.spotify),
        artwork_url: album.and_then(|value| first_image(&value.images)),
        artists: track.artists.into_iter().map(|artist| (artist.name, artist.uri)).collect(),
        album: album
            .and_then(|value| value.uri.as_ref().map(|uri| (value.name.clone(), uri.clone()))),
        duration_ms: track.duration_ms,
        available: track.is_playable.unwrap_or(true) && !track.is_local && supported,
        position,
        saved: None,
    }
}

fn browse_album(album: AlbumObject) -> SpotifyBrowseItem {
    let uri = album.uri.clone();
    SpotifyBrowseItem {
        id: id_from_uri(uri.as_deref(), &album.name),
        kind: SpotifyEntityKind::Album,
        title: album.name,
        subtitle: artist_names(&album.artists),
        metadata: String::new(),
        uri,
        external_url: album.external_urls.and_then(|urls| urls.spotify),
        artwork_url: first_image(&album.images),
        artists: album.artists.into_iter().map(|artist| (artist.name, artist.uri)).collect(),
        album: None,
        duration_ms: None,
        available: true,
        position: None,
        saved: None,
    }
}

fn browse_artist(artist: ArtistObject) -> SpotifyBrowseItem {
    let uri = artist.uri.clone();
    SpotifyBrowseItem {
        id: id_from_uri(uri.as_deref(), &artist.name),
        kind: SpotifyEntityKind::Artist,
        title: artist.name,
        subtitle: artist.genres.join(" · "),
        metadata: String::new(),
        uri,
        external_url: artist.external_urls.and_then(|urls| urls.spotify),
        artwork_url: first_image(&artist.images),
        artists: Vec::new(),
        album: None,
        duration_ms: None,
        available: true,
        position: None,
        saved: None,
    }
}

fn browse_playlist(playlist: PlaylistObject) -> SpotifyBrowseItem {
    let uri = playlist.uri.clone();
    SpotifyBrowseItem {
        id: id_from_uri(uri.as_deref(), &playlist.name),
        kind: SpotifyEntityKind::Playlist,
        title: playlist.name,
        subtitle: playlist
            .items
            .map_or_else(|| "Playlist".into(), |items| format!("{} tracks", items.total)),
        metadata: String::new(),
        uri,
        external_url: playlist.external_urls.and_then(|urls| urls.spotify),
        artwork_url: first_image(&playlist.images),
        artists: Vec::new(),
        album: None,
        duration_ms: None,
        available: true,
        position: None,
        saved: None,
    }
}

fn browse_display(
    item: SpotifyDisplayItem,
    kind: SpotifyEntityKind,
    position: Option<usize>,
) -> SpotifyBrowseItem {
    SpotifyBrowseItem {
        id: id_from_uri(item.uri.as_deref(), &item.title),
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
        position,
        saved: None,
    }
}

fn normalize_playlist_items(
    page: PlaylistItemsPage,
    fallback_offset: u32,
    fallback_limit: u32,
) -> SpotifyPage<SpotifyBrowseItem> {
    let offset = page.offset.unwrap_or(fallback_offset);
    let mut items = Vec::new();
    for (relative, entry) in page.items.into_iter().enumerate() {
        let Some(track) = entry.and_then(PlaylistItemObject::into_track) else {
            continue;
        };
        let absolute = usize::try_from(offset).unwrap_or(usize::MAX).saturating_add(relative);
        let mut item = browse_track(track, Some(absolute));
        item.metadata = format!("Playlist position {}", absolute + 1);
        items.push(item);
    }
    SpotifyPage {
        items,
        offset,
        limit: page.limit.unwrap_or(fallback_limit),
        total: page.total,
        next_offset: page.next.map(|_| offset.saturating_add(page.limit.unwrap_or(fallback_limit))),
    }
}

fn artist_names(artists: &[ArtistObject]) -> String {
    artists.iter().map(|artist| artist.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn first_image(images: &[ImageObject]) -> Option<String> {
    images.first().map(|image| image.url.clone())
}

fn status_error(status: StatusCode) -> AppError {
    let (kind, message) = match status.as_u16() {
        401 => (ErrorKind::Authentication, "Spotify session expired"),
        403 => {
            (ErrorKind::Restricted, "This action is unavailable for this Spotify account or app")
        }
        404 => (
            ErrorKind::Unavailable,
            "No active Spotify device or requested item was found; open Spotify on a device first",
        ),
        429 => (ErrorKind::RateLimited, "Spotify request limit reached; try again shortly"),
        500..=599 => (ErrorKind::Unavailable, "Spotify is temporarily unavailable"),
        _ => (ErrorKind::Network, "Spotify could not complete this request"),
    };
    AppError::new(kind, format!("{message} (HTTP {})", status.as_u16()))
}

fn response_error(response: &reqwest::blocking::Response) -> AppError {
    if response.status() == StatusCode::TOO_MANY_REQUESTS
        && let Some(seconds) = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    {
        return AppError::rate_limited(
            format!("Spotify request limit reached; retry in {seconds} seconds"),
            Duration::from_secs(seconds),
        );
    }
    status_error(response.status())
}

fn registration_error(response: &reqwest::blocking::Response) -> AppError {
    let mut error = response_error(response);
    if response.status() == StatusCode::NOT_FOUND {
        error.kind = ErrorKind::NotFound;
    }
    error
}

fn network_error(error: reqwest::Error) -> AppError {
    let message = if error.is_timeout() {
        "Spotify request timed out".to_string()
    } else {
        error.to_string()
    };
    drop(error);
    AppError::new(ErrorKind::Network, message)
}

fn invalid_data(error: &reqwest::Error) -> AppError {
    AppError::new(ErrorKind::Network, format!("Spotify returned invalid data: {error}"))
}

#[derive(Serialize)]
struct PlayBody<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    uris: Option<Vec<&'a str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_uri: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset: Option<PlayOffset>,
}

#[derive(Serialize)]
struct PlayOffset {
    position: u32,
}

#[derive(Serialize)]
struct TransferBody<'a> {
    device_ids: [&'a str; 1],
    play: bool,
}

#[derive(Deserialize)]
struct PlaybackResponse {
    #[serde(default)]
    is_playing: bool,
    progress_ms: Option<u64>,
    item: Option<TrackObject>,
    device: Option<DeviceObject>,
    context: Option<PlaybackContext>,
    #[serde(default)]
    shuffle_state: bool,
    repeat_state: Option<String>,
    actions: Option<PlaybackActions>,
}

#[derive(Deserialize)]
struct PlaybackContext {
    uri: Option<String>,
}

#[derive(Deserialize)]
struct PlaybackActions {
    #[serde(default)]
    disallows: std::collections::HashMap<String, bool>,
}

impl PlaybackActions {
    fn available(self) -> Vec<String> {
        [
            "pausing",
            "resuming",
            "seeking",
            "skipping_prev",
            "skipping_next",
            "toggling_shuffle",
            "setting_repeat_context",
            "setting_repeat_track",
            "transferring_playback",
        ]
        .into_iter()
        .filter(|action| !self.disallows.get(*action).copied().unwrap_or(false))
        .map(str::to_owned)
        .collect()
    }
}

#[derive(Deserialize)]
struct DevicesResponse {
    #[serde(default)]
    devices: Vec<DeviceObject>,
}

#[derive(Deserialize)]
struct DeviceObject {
    id: Option<String>,
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_restricted: bool,
    volume_percent: Option<u8>,
}

#[derive(Deserialize)]
struct QueueResponse {
    currently_playing: Option<TrackObject>,
    #[serde(default)]
    queue: Vec<TrackObject>,
}

#[derive(Deserialize)]
struct HistoryPage {
    #[serde(default)]
    items: Vec<HistoryItem>,
}

#[derive(Deserialize)]
struct HistoryItem {
    track: TrackObject,
}

#[derive(Default, Deserialize)]
struct TrackPage {
    #[serde(default)]
    items: Vec<TrackObject>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct TrackObject {
    name: String,
    uri: Option<String>,
    album: Option<AlbumObject>,
    duration_ms: Option<u64>,
    #[serde(default)]
    artists: Vec<ArtistObject>,
    #[serde(default)]
    is_local: bool,
    is_playable: Option<bool>,
    disc_number: Option<u32>,
    track_number: Option<u32>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct ArtistPage {
    #[serde(default)]
    items: Vec<ArtistObject>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
}

#[derive(Deserialize)]
struct ArtistObject {
    name: String,
    uri: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    #[serde(default)]
    genres: Vec<String>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct AlbumPage {
    #[serde(default)]
    items: Vec<SavedAlbum>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct SavedAlbum {
    album: AlbumObject,
}

#[derive(Deserialize)]
struct AlbumObject {
    name: String,
    uri: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    #[serde(default)]
    artists: Vec<ArtistObject>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct AlbumDetailObject {
    name: String,
    uri: Option<String>,
    release_date: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    #[serde(default)]
    artists: Vec<ArtistObject>,
    tracks: Option<TrackPage>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct PlaylistPage {
    #[serde(default)]
    items: Vec<PlaylistObject>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct PlaylistObject {
    name: String,
    uri: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    items: Option<PlaylistItems>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct PlaylistDetailObject {
    name: String,
    uri: Option<String>,
    description: Option<String>,
    #[serde(default)]
    images: Vec<ImageObject>,
    items: Option<PlaylistItems>,
    owner: Option<OwnerObject>,
    external_urls: Option<ExternalUrls>,
}

#[derive(Deserialize)]
struct OwnerObject {
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct ExternalUrls {
    spotify: Option<String>,
}

#[derive(Deserialize)]
struct PlaylistItemsPage {
    #[serde(default)]
    items: Vec<Option<PlaylistItemObject>>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct PlaylistItemObject {
    item: Option<TrackObject>,
    track: Option<TrackObject>,
}

impl PlaylistItemObject {
    fn into_track(self) -> Option<TrackObject> {
        self.item.or(self.track)
    }
}

#[derive(Deserialize)]
struct ImageObject {
    url: String,
}

#[derive(Deserialize)]
struct PlaylistItems {
    total: u32,
}

#[derive(Deserialize)]
struct SearchResponse {
    tracks: Option<TrackPage>,
    albums: Option<SearchAlbumPage>,
    artists: Option<ArtistPage>,
    playlists: Option<SearchPlaylistPage>,
}

#[derive(Deserialize)]
struct SearchAlbumPage {
    #[serde(default)]
    items: Vec<AlbumObject>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
}

#[derive(Serialize)]
struct IdsBody<'a> {
    ids: &'a [&'a str],
}

#[derive(Serialize)]
struct AddPlaylistTracksBody<'a> {
    uris: &'a [&'a str],
}

#[derive(Serialize)]
struct TrackUriObject<'a> {
    uri: &'a str,
}

#[derive(Serialize)]
struct RemovePlaylistTracksBody<'a> {
    tracks: Vec<TrackUriObject<'a>>,
}

#[derive(Serialize)]
struct UpdatePlaylistBody<'a> {
    name: &'a str,
}

#[derive(Deserialize)]
struct SnapshotResponse {
    #[allow(dead_code)]
    snapshot_id: Option<String>,
}

#[derive(Deserialize)]
struct SavedTracksPage {
    #[serde(default)]
    items: Vec<SavedTrackItem>,
    #[serde(default)]
    total: u32,
    #[allow(dead_code)]
    next: Option<String>,
    offset: Option<u32>,
    limit: Option<u32>,
}

#[derive(Deserialize)]
struct SavedTrackItem {
    track: Option<TrackObject>,
}

fn track_object_to_core(track: TrackObject) -> Result<Track> {
    let uri_str =
        track.uri.ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "missing track uri"))?;
    let uri: SpotifyUri =
        uri_str.parse().map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid track uri"))?;
    let id = TrackId::parse(uri.id())
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid track id"))?;
    let artists = track
        .artists
        .into_iter()
        .filter_map(|a| {
            let a_uri = a.uri.as_deref().and_then(|u| u.parse::<SpotifyUri>().ok())?;
            let a_id = ArtistId::parse(a_uri.id()).ok()?;
            Some(ArtistSummary {
                id: a_id,
                name: a.name,
                external_url: a.external_urls.and_then(|u| u.spotify).unwrap_or_default(),
            })
        })
        .collect();
    let album = if let Some(alb) = track.album {
        album_object_to_summary(alb)?
    } else {
        return Err(AppError::new(ErrorKind::InvalidInput, "track has no album"));
    };
    Ok(Track {
        id,
        uri,
        name: track.name,
        artists,
        album,
        duration: Duration::from_millis(track.duration_ms.unwrap_or(0)),
        explicit: false,
        playable: track.is_playable.unwrap_or(true),
        external_url: track.external_urls.and_then(|u| u.spotify).unwrap_or_default(),
    })
}

fn album_object_to_summary(album: AlbumObject) -> Result<AlbumSummary> {
    let uri_str =
        album.uri.ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "missing album uri"))?;
    let uri: SpotifyUri =
        uri_str.parse().map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid album uri"))?;
    let id = AlbumId::parse(uri.id())
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "invalid album id"))?;
    let artists = album
        .artists
        .into_iter()
        .filter_map(|a| {
            let a_uri = a.uri.as_deref().and_then(|u| u.parse::<SpotifyUri>().ok())?;
            let a_id = ArtistId::parse(a_uri.id()).ok()?;
            Some(ArtistSummary {
                id: a_id,
                name: a.name,
                external_url: a.external_urls.and_then(|u| u.spotify).unwrap_or_default(),
            })
        })
        .collect();
    let artwork = album
        .images
        .into_iter()
        .map(|img| Image { url: img.url, width: None, height: None })
        .collect();
    Ok(AlbumSummary {
        id,
        name: album.name,
        artists,
        artwork,
        external_url: album.external_urls.and_then(|u| u.spotify).unwrap_or_default(),
    })
}

#[derive(Clone, Debug)]
pub struct SpotifyLibraryService {
    api: SpotifyWebApi,
    token: String,
}

impl SpotifyLibraryService {
    #[must_use]
    pub fn new(api: SpotifyWebApi, token: String) -> Self {
        Self { api, token }
    }
}

impl LibraryService for SpotifyLibraryService {
    fn liked_tracks(&self, offset: u32, limit: u32) -> BoxFuture<'_, Result<Page<Track>>> {
        let page = self.api.liked_tracks_core(&self.token, offset, limit);
        Box::pin(std::future::ready(page))
    }

    fn saved_albums(&self, offset: u32, limit: u32) -> BoxFuture<'_, Result<Page<AlbumSummary>>> {
        let page = self.api.saved_albums_core(&self.token, offset, limit);
        Box::pin(std::future::ready(page))
    }

    fn save(&self, uris: Vec<SpotifyUri>) -> BoxFuture<'_, Result<()>> {
        let str_uris: Vec<String> = uris.into_iter().map(String::from).collect();
        let refs: Vec<&str> = str_uris.iter().map(String::as_str).collect();
        let result = self.api.save_library_items(&self.token, &refs);
        Box::pin(std::future::ready(result))
    }

    fn remove(&self, uris: Vec<SpotifyUri>) -> BoxFuture<'_, Result<()>> {
        let str_uris: Vec<String> = uris.into_iter().map(String::from).collect();
        let refs: Vec<&str> = str_uris.iter().map(String::as_str).collect();
        let result = self.api.remove_library_items(&self.token, &refs);
        Box::pin(std::future::ready(result))
    }

    fn contains(&self, uris: Vec<SpotifyUri>) -> BoxFuture<'_, Result<Vec<bool>>> {
        let str_uris: Vec<String> = uris.into_iter().map(String::from).collect();
        let refs: Vec<&str> = str_uris.iter().map(String::as_str).collect();
        let result = self.api.contains_library_items(&self.token, &refs);
        Box::pin(std::future::ready(result))
    }
}

#[derive(Deserialize)]
struct SearchPlaylistPage {
    #[serde(default)]
    items: Vec<Option<PlaylistObject>>,
    #[serde(default)]
    total: u32,
    next: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_playlist_shape_uses_items_count() {
        let page: PlaylistPage =
            serde_json::from_str(r#"{"items":[{"name":"Focus","items":{"total":12}}]}"#).unwrap();
        assert_eq!(page.items[0].items.as_ref().unwrap().total, 12);
    }

    #[test]
    fn removed_artist_fields_are_optional() {
        let page: ArtistPage = serde_json::from_str(r#"{"items":[{"name":"Rosalía"}]}"#).unwrap();
        assert!(page.items[0].genres.is_empty());
    }

    #[test]
    fn album_detail_shape_reads_tracks_and_release_date() {
        let album: AlbumDetailObject = serde_json::from_str(
            r#"{
                "name":"Motomami",
                "uri":"spotify:album:abc",
                "release_date":"2022-03-18",
                "artists":[{"name":"Rosalía"}],
                "tracks":{"items":[
                    {"name":"Saoko","uri":"spotify:track:1"},
                    {"name":"Candy","uri":"spotify:track:2"}
                ]}
            }"#,
        )
        .unwrap();
        assert_eq!(album.name, "Motomami");
        assert_eq!(album.release_date.as_deref(), Some("2022-03-18"));
        assert_eq!(album.tracks.unwrap().items.len(), 2);
    }

    #[test]
    fn search_tolerates_unavailable_playlist_entries() {
        let response: SearchResponse = serde_json::from_str(
            r#"{"tracks":{"items":[]},"playlists":{"items":[null,{"name":"Mix","items":null}]}}"#,
        )
        .unwrap();
        assert_eq!(response.playlists.unwrap().items.len(), 2);
    }

    #[test]
    fn playback_shape_keeps_device_and_track_timing() {
        let response: PlaybackResponse = serde_json::from_str(
            r#"{
                "is_playing":true,
                "progress_ms":42000,
                "device":{"id":"speaker","name":"Office","type":"Speaker","volume_percent":65},
                "item":{"name":"Song","duration_ms":180000,"artists":[{"name":"Artist"}]}
            }"#,
        )
        .unwrap();
        assert!(response.is_playing);
        assert_eq!(response.progress_ms, Some(42_000));
        assert_eq!(response.device.unwrap().volume_percent, Some(65));
        assert_eq!(response.item.unwrap().duration_ms, Some(180_000));
    }

    #[test]
    fn queue_shape_tolerates_episode_like_items() {
        let response: QueueResponse = serde_json::from_str(
            r#"{"currently_playing":null,"queue":[{"name":"Episode","uri":"spotify:episode:abc"}]}"#,
        )
        .unwrap();
        assert_eq!(response.queue.len(), 1);
        assert!(response.queue[0].artists.is_empty());
    }

    #[test]
    fn playlist_items_normalize_variants_nulls_and_duplicate_positions() {
        let page: PlaylistItemsPage = serde_json::from_str(
            r#"{
            "offset":50,"limit":50,"total":103,"next":"next-page","items":[
                null,
                {"item":{"name":"Again","uri":"spotify:track:same","duration_ms":1000}},
                {"track":{"name":"Again","uri":"spotify:track:same","duration_ms":1000}},
                {"item":null}
            ]
        }"#,
        )
        .unwrap();
        let normalized = normalize_playlist_items(page, 0, 50);
        assert_eq!(normalized.items.len(), 2);
        assert_eq!(normalized.items[0].position, Some(51));
        assert_eq!(normalized.items[1].position, Some(52));
        assert_eq!(normalized.items[0].uri, normalized.items[1].uri);
        assert_eq!(normalized.next_offset, Some(100));
    }

    #[test]
    fn page_shape_preserves_offsets_beyond_fifty() {
        let page: AlbumPage = serde_json::from_str(
            r#"{"offset":50,"limit":50,"total":120,"next":"page-3","items":[]}"#,
        )
        .unwrap();
        assert_eq!(page.offset, Some(50));
        assert_eq!(page.total, 120);
        assert!(page.next.is_some());
    }

    #[test]
    fn context_playback_serializes_positional_offset() {
        let body = PlayBody {
            uris: None,
            context_uri: Some("spotify:playlist:abc"),
            offset: Some(PlayOffset { position: 7 }),
        };
        let value = serde_json::to_value(body).unwrap();
        assert_eq!(value["offset"]["position"], 7);
        assert_eq!(value["context_uri"], "spotify:playlist:abc");
    }

    #[test]
    fn liked_tracks_shape_parses_items_and_tolerates_null_track() {
        let page: SavedTracksPage = serde_json::from_str(
            r#"{
                "items": [
                    {"track": {"name": "Let Down", "uri": "spotify:track:123", "duration_ms": 299000}},
                    {"track": null}
                ],
                "total": 1,
                "limit": 50,
                "offset": 0
            }"#,
        )
        .unwrap();
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].track.as_ref().unwrap().name, "Let Down");
        assert!(page.items[1].track.is_none());
    }

    #[test]
    fn check_saved_tracks_parses_boolean_array() {
        let bools: Vec<bool> = serde_json::from_str(r"[true, false, true]").unwrap();
        assert_eq!(bools, vec![true, false, true]);
    }

    #[test]
    fn playlist_mutation_bodies_serialize_expected_keys() {
        let add = AddPlaylistTracksBody { uris: &["spotify:track:1", "spotify:track:2"] };
        let val = serde_json::to_value(add).unwrap();
        assert_eq!(val["uris"], serde_json::json!(["spotify:track:1", "spotify:track:2"]));

        let remove =
            RemovePlaylistTracksBody { tracks: vec![TrackUriObject { uri: "spotify:track:1" }] };
        let val = serde_json::to_value(remove).unwrap();
        assert_eq!(val["tracks"][0]["uri"], "spotify:track:1");
    }
}
