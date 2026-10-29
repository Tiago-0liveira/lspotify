use std::sync::Mutex;

use lspotify_core::{
    AppError, ErrorKind, LocalPlaybackBackendPreference, LocalPlayerCommand, LocalPlayerEvent,
    Result,
};
use lspotify_playback::LibrespotLocalPlayer;

#[cfg(target_os = "windows")]
use std::{
    io::{BufRead as _, BufReader, Write as _},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
};

#[derive(Debug)]
enum ActiveBackend {
    Librespot(LibrespotLocalPlayer),
    #[cfg(target_os = "windows")]
    SpotifyWeb(WebPlaybackHost),
}

impl ActiveBackend {
    fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        match self {
            Self::Librespot(player) => player.send(command),
            #[cfg(target_os = "windows")]
            Self::SpotifyWeb(player) => player.send(command),
        }
    }

    fn try_next_event(&self) -> Result<Option<LocalPlayerEvent>> {
        match self {
            Self::Librespot(player) => player.try_next_event(),
            #[cfg(target_os = "windows")]
            Self::SpotifyWeb(player) => player.try_next_event(),
        }
    }

    fn shutdown(&self) -> Result<()> {
        match self {
            Self::Librespot(player) => player.shutdown(),
            #[cfg(target_os = "windows")]
            Self::SpotifyWeb(player) => player.shutdown(),
        }
    }

    fn pid(&self) -> Option<u32> {
        match self {
            Self::Librespot(_) => None,
            #[cfg(target_os = "windows")]
            Self::SpotifyWeb(player) => Some(player.pid),
        }
    }
}

/// Lazy local playback manager.
///
/// Constructing this type does not initialize Spotify audio, `WebView2`, or librespot. The selected
/// backend is created only when a `Connect` command is sent.
#[derive(Debug)]
pub struct BackgroundLocalPlayer {
    backend: Mutex<LocalPlaybackBackendPreference>,
    active: Mutex<Option<ActiveBackend>>,
    access_token: Mutex<Option<String>>,
    ready: Mutex<bool>,
    device_id: Mutex<Option<String>>,
    shutdown_reason: Mutex<Option<String>>,
}

impl BackgroundLocalPlayer {
    pub fn start(backend: LocalPlaybackBackendPreference) -> Self {
        Self {
            backend: Mutex::new(backend),
            active: Mutex::new(None),
            access_token: Mutex::new(None),
            ready: Mutex::new(false),
            device_id: Mutex::new(None),
            shutdown_reason: Mutex::new(None),
        }
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn backend(&self) -> Result<LocalPlaybackBackendPreference> {
        self.backend.lock().map_err(|_| poisoned()).map(|backend| *backend)
    }

    /// Changes the local backend. Any running local engine is stopped; the replacement remains
    /// lazy until local playback is requested again.
    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn set_backend(&self, backend: LocalPlaybackBackendPreference) -> Result<()> {
        let current = self.backend()?;
        if current == backend {
            return Ok(());
        }
        self.stop_active()?;
        *self.backend.lock().map_err(|_| poisoned())? = backend;
        *self.ready.lock().map_err(|_| poisoned())? = false;
        *self.device_id.lock().map_err(|_| poisoned())? = None;
        *self.shutdown_reason.lock().map_err(|_| poisoned())? = None;
        Ok(())
    }

    /// Sends a typed local-player command. `TokenUpdate` is cached without starting an engine;
    /// `Connect` performs the lazy initialization.
    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        if let LocalPlayerCommand::TokenUpdate { access_token } = &command {
            *self.access_token.lock().map_err(|_| poisoned())? = Some(access_token.clone());
            if let Some(active) = self.active.lock().map_err(|_| poisoned())?.as_ref() {
                active.send(command)?;
            }
            return Ok(());
        }

        if matches!(command, LocalPlayerCommand::Connect) {
            self.ensure_started()?;
        }

        if matches!(command, LocalPlayerCommand::Shutdown) {
            return self.shutdown();
        }

        let active = self.active.lock().map_err(|_| poisoned())?;
        let Some(active) = active.as_ref() else {
            return Err(AppError::new(
                ErrorKind::Unavailable,
                "local playback has not been started",
            ));
        };
        active.send(command)
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn try_next_event(&self) -> Result<Option<LocalPlayerEvent>> {
        let maybe_event = {
            let active = self.active.lock().map_err(|_| poisoned())?;
            active.as_ref().map(ActiveBackend::try_next_event).transpose()?.flatten()
        };

        if let Some(event) = maybe_event {
            if let LocalPlayerEvent::Ready { ref device_id } = event {
                *self.ready.lock().map_err(|_| poisoned())? = true;
                *self.device_id.lock().map_err(|_| poisoned())? = Some(device_id.to_string());
                *self.shutdown_reason.lock().map_err(|_| poisoned())? = None;
            } else if matches!(
                event,
                LocalPlayerEvent::Unavailable
                    | LocalPlayerEvent::AuthenticationError(_)
                    | LocalPlayerEvent::AccountError(_)
            ) {
                *self.ready.lock().map_err(|_| poisoned())? = false;
                *self.device_id.lock().map_err(|_| poisoned())? = None;
            }
            return Ok(Some(event));
        }
        Ok(None)
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn is_ready(&self) -> Result<bool> {
        self.ready.lock().map_err(|_| poisoned()).map(|ready| *ready)
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn pid(&self) -> Result<Option<u32>> {
        let active = self.active.lock().map_err(|_| poisoned())?;
        Ok(active.as_ref().and_then(ActiveBackend::pid))
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn device_id(&self) -> Result<Option<String>> {
        self.device_id.lock().map_err(|_| poisoned()).map(|id| id.clone())
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn shutdown_reason(&self) -> Result<Option<String>> {
        self.shutdown_reason.lock().map_err(|_| poisoned()).map(|reason| reason.clone())
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn shutdown_with_reason(&self, reason: &str) -> Result<()> {
        {
            let mut shutdown_reason = self.shutdown_reason.lock().map_err(|_| poisoned())?;
            if shutdown_reason.is_none() {
                *shutdown_reason = Some(reason.to_owned());
            }
        }
        self.stop_active()?;
        *self.ready.lock().map_err(|_| poisoned())? = false;
        *self.device_id.lock().map_err(|_| poisoned())? = None;
        Ok(())
    }

    /// # Errors
    /// Returns an error if a player lock is poisoned or the backend operation fails.
    pub fn shutdown(&self) -> Result<()> {
        self.shutdown_with_reason("normal shutdown")
    }

    fn ensure_started(&self) -> Result<()> {
        let mut active = self.active.lock().map_err(|_| poisoned())?;
        if active.is_some() {
            return Ok(());
        }

        let backend = self.backend()?;
        let player = match backend {
            LocalPlaybackBackendPreference::Librespot => {
                let credentials_dir = crate::AppPaths::discover()?.cache.join("librespot");
                ActiveBackend::Librespot(LibrespotLocalPlayer::start(credentials_dir)?)
            }
            LocalPlaybackBackendPreference::SpotifyWeb => {
                #[cfg(target_os = "windows")]
                {
                    ActiveBackend::SpotifyWeb(WebPlaybackHost::start()?)
                }
                #[cfg(not(target_os = "windows"))]
                {
                    return Err(AppError::new(
                        ErrorKind::Unavailable,
                        "Spotify Web Playback SDK is available only on Windows",
                    ));
                }
            }
        };

        if let Some(token) = self.access_token.lock().map_err(|_| poisoned())?.clone() {
            player.send(LocalPlayerCommand::TokenUpdate { access_token: token })?;
        }
        *active = Some(player);
        Ok(())
    }

    fn stop_active(&self) -> Result<()> {
        let active = self.active.lock().map_err(|_| poisoned())?.take();
        if let Some(active) = active {
            active.shutdown()?;
        }
        Ok(())
    }
}

impl Drop for BackgroundLocalPlayer {
    fn drop(&mut self) {
        if let Ok(active) = self.active.get_mut()
            && let Some(active) = active.take()
        {
            let _ = active.shutdown();
        }
    }
}

#[cfg(target_os = "windows")]
#[derive(Debug)]
struct WebPlaybackHost {
    commands: mpsc::SyncSender<LocalPlayerCommand>,
    events: Mutex<mpsc::Receiver<LocalPlayerEvent>>,
    child: Mutex<Option<Child>>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    pid: u32,
}

#[cfg(target_os = "windows")]
impl WebPlaybackHost {
    fn start() -> Result<Self> {
        let executable = locate_player_host()?;
        let mut child = Command::new(executable)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| io_error(&error))?;
        let pid = child.id();
        tracing::info!(pid, "spawned Web Playback SDK player-host");

        let (commands, command_receiver) = mpsc::sync_channel(64);
        let (event_sender, events) = mpsc::sync_channel(64);
        let shutdown_reason = Arc::new(Mutex::new(None));
        let mut threads = Vec::new();

        if let Some(mut stdin) = child.stdin.take() {
            threads.push(std::thread::spawn(move || {
                for command in command_receiver {
                    let shutdown = matches!(command, LocalPlayerCommand::Shutdown);
                    let line = local_command_json(command);
                    if writeln!(stdin, "{line}").is_err() || stdin.flush().is_err() {
                        break;
                    }
                    if shutdown {
                        break;
                    }
                }
            }));
        }

        if let Some(stdout) = child.stdout.take() {
            let reason = Arc::clone(&shutdown_reason);
            threads.push(std::thread::spawn(move || {
                forward_host_events(stdout, &event_sender, reason.as_ref());
            }));
        }

        Ok(Self {
            commands,
            events: Mutex::new(events),
            child: Mutex::new(Some(child)),
            threads: Mutex::new(threads),
            pid,
        })
    }

    fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        self.commands.try_send(command).map_err(|_| {
            AppError::new(
                ErrorKind::Unavailable,
                "local Web Playback SDK queue is full or the host has stopped",
            )
        })
    }

    fn try_next_event(&self) -> Result<Option<LocalPlayerEvent>> {
        self.events
            .lock()
            .map_err(|_| poisoned())?
            .try_recv()
            .ok()
            .map_or(Ok(None), |event| Ok(Some(event)))
    }

    fn shutdown(&self) -> Result<()> {
        let _ = self.commands.try_send(LocalPlayerCommand::Shutdown);
        if let Some(mut child) = self.child.lock().map_err(|_| poisoned())?.take() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            let mut exited = false;
            while std::time::Instant::now() < deadline {
                if child.try_wait().map_err(|error| io_error(&error))?.is_some() {
                    exited = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if !exited {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        let handles = std::mem::take(&mut *self.threads.lock().map_err(|_| poisoned())?);
        for handle in handles {
            let _ = handle.join();
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn locate_player_host() -> Result<PathBuf> {
    let executable = std::env::var_os("LSPOTIFY_PLAYER_HOST").map(PathBuf::from).or_else(|| {
        std::env::current_exe().ok().and_then(|path| {
            let directory = path.parent()?;
            let direct = directory.join("lspotify-player-host.exe");
            if direct.is_file() {
                return Some(direct);
            }
            let helper = directory.join("helpers").join("lspotify-player-host.exe");
            helper.is_file().then_some(helper)
        })
    });
    executable.filter(|path| path.is_file()).ok_or_else(|| {
        AppError::new(
            ErrorKind::Unavailable,
            "lspotify's Web Playback SDK helper is missing; reinstall the application",
        )
    })
}

#[cfg(target_os = "windows")]
fn forward_host_events(
    stdout: impl std::io::Read,
    sender: &mpsc::SyncSender<LocalPlayerEvent>,
    shutdown_reason: &Mutex<Option<String>>,
) {
    for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
        if let Ok(envelope) = lspotify_playback::parse_event(&line) {
            let event = match envelope.payload {
                lspotify_playback::BridgeEvent::Ready { device_id } => {
                    LocalPlayerEvent::Ready { device_id }
                }
                lspotify_playback::BridgeEvent::Unavailable { .. } => LocalPlayerEvent::Unavailable,
                lspotify_playback::BridgeEvent::StateChanged {
                    playing,
                    position_ms,
                    duration_ms,
                    track_uri,
                    title,
                    artist,
                    album,
                    artwork_url,
                } => LocalPlayerEvent::StateChanged {
                    playing,
                    position_ms,
                    duration_ms,
                    track_uri,
                    title,
                    artist,
                    album,
                    artwork_url,
                },
                lspotify_playback::BridgeEvent::AuthenticationError { message } => {
                    LocalPlayerEvent::AuthenticationError(message)
                }
                lspotify_playback::BridgeEvent::PlaybackError { message } => {
                    LocalPlayerEvent::PlaybackError(message)
                }
                lspotify_playback::BridgeEvent::AccountError { message } => {
                    LocalPlayerEvent::AccountError(message)
                }
            };
            let _ = sender.try_send(event);
        }
    }

    if let Ok(mut reason) = shutdown_reason.lock()
        && reason.is_none()
    {
        *reason = Some("player-host process exited unexpectedly".to_owned());
    }
    let _ = sender.try_send(LocalPlayerEvent::Unavailable);
}

#[cfg(target_os = "windows")]
fn local_command_json(command: LocalPlayerCommand) -> String {
    use serde_json::json;
    match command {
        LocalPlayerCommand::TokenUpdate { access_token } => {
            json!({"type":"token_update","access_token":access_token})
        }
        LocalPlayerCommand::Connect => json!({"type":"connect"}),
        LocalPlayerCommand::Disconnect => json!({"type":"disconnect"}),
        LocalPlayerCommand::Play => json!({"type":"play"}),
        LocalPlayerCommand::Pause => json!({"type":"pause"}),
        LocalPlayerCommand::Previous => json!({"type":"previous"}),
        LocalPlayerCommand::Next => json!({"type":"next"}),
        LocalPlayerCommand::Seek { position_ms } => {
            json!({"type":"seek","position_ms":position_ms})
        }
        LocalPlayerCommand::Volume { value_milli } => {
            json!({"type":"volume","value_milli":value_milli})
        }
        LocalPlayerCommand::Shuffle { enabled } => json!({"type":"shuffle","enabled":enabled}),
        LocalPlayerCommand::Repeat { mode } => json!({"type":"repeat","mode":mode}),
        LocalPlayerCommand::Shutdown => json!({"type":"shutdown"}),
    }
    .to_string()
}

#[cfg(target_os = "windows")]
fn io_error(error: &std::io::Error) -> AppError {
    AppError::new(ErrorKind::Unavailable, error.to_string())
}

fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "local player lock was poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_is_lazy_until_connect() {
        let player = BackgroundLocalPlayer::start(LocalPlaybackBackendPreference::Librespot);
        player.send(LocalPlayerCommand::TokenUpdate { access_token: "token".into() }).unwrap();
        assert_eq!(player.pid().unwrap(), None);
        assert!(!player.is_ready().unwrap());
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn web_backend_is_rejected_off_windows() {
        let player = BackgroundLocalPlayer::start(LocalPlaybackBackendPreference::SpotifyWeb);
        player.send(LocalPlayerCommand::TokenUpdate { access_token: "token".into() }).unwrap();
        assert!(player.send(LocalPlayerCommand::Connect).is_err());
    }
}
