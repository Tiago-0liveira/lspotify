# Playback queue investigation

Investigated from `fix/librespot-startup-diagnostics` at `b8a70aa` in the
`fix/playback-context-queue` worktree branch. This is a code investigation;
no authenticated playback session was used. The findings below describe the starting
branch; the implementation update at the end records the fix.

## Findings

1. **Individual songs from Search, Home, and artist song lists start a one-track
   playback list.** `action.rs::activate` uses `Effect::PlayTrack` when the selected
   item has no `ContextPosition`. Search and Home items have no context; artist
   songs intentionally receive no context in `service.rs::apply_detail`.
   `SpotifyWebApi::play_uri_on_device` then sends
   `{"uris":["spotify:track:..."]}`. The other songs displayed on the page never
   reach the playback device. This explains a missing continuation when starting
   from those lists, independently of the librespot startup changes.

2. **Liked Songs supplies an unsupported context.** The library loader attaches
   `spotify:collection:tracks` and a position to each liked track. Activation
   dispatches `PlayContext`, but `SpotifyUri`/`play_context_at` reject that URI
   before sending a playback request. The optimistic UI already updates the
   current track before this failure, which can briefly suggest playback started.

3. **Album and playlist tracks already preserve the complete context.** They send
   a context URI plus the original zero-based track position. This lets Spotify
   resolve the remaining tracks, including tracks on pages the UI has not fetched.
   If the reported issue is specifically on these pages, the two findings above
   do not establish its cause; a concrete album/playlist and playback device are
   needed to trace that case.

4. **The queue view displays the queue response without reconstructing the full
   collection.** `typed_queue` maps every item returned by `/me/player/queue` and
   the app stores that result. It does not paginate that endpoint or synthesize
   the rest of the source collection. A short displayed queue alone does not
   establish that context playback will end early.

## API contract and repair direction

Spotify documents album, artist, and playlist playback contexts, or an explicit
array of track URIs. Positional context offsets apply to albums and playlists.
See https://developer.spotify.com/documentation/web-api/reference/start-a-users-playback.

For Search/Home/artist song lists, continuing through the selected list requires
an explicit ordered track list, or a deliberately chosen supported context.
Liked Songs needs a separate explicit-list playback path; simply accepting the
collection URI locally does not make it a documented Web API context. Its
pagination must be accounted for so playback is not limited to the loaded page.
Album and playlist playback should retain its existing context and positional
offset behavior, including duplicate tracks.

Automatic recommended songs after a list ends are a separate behavior. There is
no recommendation queue builder or autoplay setting in this application's
playback code; this investigation does not establish what the active Spotify
device will do after its supplied list ends.

## Validation

`cargo test -p lspotify-cli --lib`: 168 passed, including the existing playlist
context/duplicate-position flow and queue refresh tests. These are unit tests,
not live Spotify verification. The existing suite does not cover continuation
from Search/Home or the rejected Liked Songs playback context.

## Implementation update

The worktree now fixes the two confirmed entry points:

- Artist song activation sends the selected song and subsequent available songs
  from the displayed list as an explicit ordered track list. Releases are excluded;
  duplicate songs keep their positions. Queue selections remain independent.
- Liked Songs playback resolves the collection locally through every `/me/tracks`
  page and supplies the selected position and remaining available tracks as an
  explicit list. A page-fetch error aborts before playback is replaced, rather
  than silently playing only a partial list.
- Explicit lists target the selected Connect device and use device-transfer retry
  when the player is unavailable. The app refreshes playback and queue after success.
- Album and playlist context playback is unchanged.

These lists begin at the selected song. This does not add recommendation/autoplay
logic or expand an artist's displayed songs into their entire discography. Loading
all Liked Songs pages may delay playback for large libraries. Spotify's actual
queue response and device acceptance still need an authenticated playback check.

Validation after the fix: 173 CLI tests and 25 Spotify API tests pass. Seven new
regressions cover artist selection and queue isolation, liked-track positions,
multiple pages, missing/unavailable items, duplicates, fetch failures, stalled
pagination, and serialized/validated explicit track lists.
