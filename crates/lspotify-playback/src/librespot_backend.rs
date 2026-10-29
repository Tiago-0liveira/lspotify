use std::{
    fmt::Write as _,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::{
    Session, SessionConfig, authentication::Credentials, cache::Cache, config::DeviceType,
};
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::{
    audio_backend,
    config::{AudioFormat, PlayerConfig},
    mixer::{self, MixerConfig},
    player::Player,
};
use lspotify_core::{AppError, DeviceId, ErrorKind, LocalPlayerCommand, LocalPlayerEvent, Result};
use rand::Rng as _;

#[derive(Debug)]
pub struct LibrespotLocalPlayer {
    commands: mpsc::SyncSender<LocalPlayerCommand>,
    stopping: Arc<AtomicBool>,
    connecting: Arc<AtomicBool>,
    events: Mutex<mpsc::Receiver<LocalPlayerEvent>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl LibrespotLocalPlayer {
    /// Starts the lightweight coordinator thread. Spotify networking and audio are initialized
    /// only after a `Connect` command is received.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker thread cannot be created.
    pub fn start(credentials_dir: PathBuf) -> Result<Self> {
        let (commands, command_receiver) = mpsc::sync_channel(64);
        let (event_sender, events) = mpsc::sync_channel(64);
        let stopping = Arc::new(AtomicBool::new(false));
        let connecting = Arc::new(AtomicBool::new(false));
        let worker_stopping = Arc::clone(&stopping);
        let worker_connecting = Arc::clone(&connecting);
        let thread = thread::Builder::new()
            .name("lspotify-librespot".into())
            .spawn(move || {
                run(
                    command_receiver,
                    event_sender,
                    credentials_dir,
                    worker_stopping,
                    worker_connecting,
                );
            })
            .map_err(|error| AppError::new(ErrorKind::Unavailable, error.to_string()))?;
        Ok(Self {
            commands,
            stopping,
            connecting,
            events: Mutex::new(events),
            thread: Mutex::new(Some(thread)),
        })
    }

    /// Queues a command without blocking the caller.
    ///
    /// # Errors
    /// Returns an error if the command queue is full or the worker stopped.
    pub fn send(&self, command: LocalPlayerCommand) -> Result<()> {
        if matches!(command, LocalPlayerCommand::Shutdown) {
            self.stopping.store(true, Ordering::Release);
            return Ok(());
        }
        let connect = matches!(command, LocalPlayerCommand::Connect);
        if connect && self.connecting.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.commands.try_send(command).map_err(|_| {
            if connect {
                self.connecting.store(false, Ordering::Release);
            }
            AppError::new(
                ErrorKind::Unavailable,
                "librespot playback queue is full or the worker has stopped",
            )
        })
    }

    /// Reads the next event without waiting.
    ///
    /// # Errors
    /// Returns an error if the event lock is poisoned.
    pub fn try_next_event(&self) -> Result<Option<LocalPlayerEvent>> {
        self.events
            .lock()
            .map_err(|_| poisoned())?
            .try_recv()
            .ok()
            .map_or(Ok(None), |event| Ok(Some(event)))
    }

    /// Stops the coordinator.
    ///
    /// # Errors
    /// Returns an error if the thread lock is poisoned.
    pub fn shutdown(&self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        if let Some(thread) = self.thread.lock().map_err(|_| poisoned())?.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}

impl Drop for LibrespotLocalPlayer {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Release);
        if let Ok(thread) = self.thread.get_mut()
            && let Some(thread) = thread.take()
        {
            let _ = thread.join();
        }
    }
}

struct ActivePlayer {
    session: Session,
    spirc: Spirc,
    _player: Arc<Player>,
    spirc_task: tokio::task::JoinHandle<()>,
    device_id: DeviceId,
    ready_at: Instant,
    ready_emitted: bool,
}

fn run(
    commands: mpsc::Receiver<LocalPlayerCommand>,
    events: mpsc::SyncSender<LocalPlayerEvent>,
    credentials_dir: PathBuf,
    stopping: Arc<AtomicBool>,
    connection_pending: Arc<AtomicBool>,
) {
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = events.try_send(LocalPlayerEvent::Unavailable);
            tracing::error!(%error, "could not create librespot runtime");
            return;
        }
    };
    runtime.block_on(run_async(
        commands,
        events,
        credentials_dir,
        stopping,
        connection_pending,
        |path| tokio::spawn(connection_with_timeout(connect(path), Duration::from_secs(120))),
    ));
}

// The command dispatch stays together so startup cancellation and active-player teardown
// share one owner; splitting it risks dropping a live SPIRC task without shutdown.
#[allow(clippy::too_many_lines)]
async fn run_async(
    commands: mpsc::Receiver<LocalPlayerCommand>,
    events: mpsc::SyncSender<LocalPlayerEvent>,
    credentials_dir: PathBuf,
    stopping: Arc<AtomicBool>,
    connection_pending: Arc<AtomicBool>,
    mut start_connection: impl FnMut(PathBuf) -> ConnectionTask,
) {
    let mut active: Option<ActivePlayer> = None;
    let mut connecting = None;

    loop {
        if stopping.load(Ordering::Acquire) {
            cancel_connection(&mut connecting, &events);
            shutdown_active(active.take());
            break;
        }
        match commands.try_recv() {
            Ok(LocalPlayerCommand::Connect) => {
                if let Some(player) = active.as_ref() {
                    connection_pending.store(false, Ordering::Release);
                    if let Err(error) = player.spirc.activate() {
                        playback_error(&events, error.to_string());
                    }
                } else if connecting.is_none() {
                    connecting = Some(start_connection(credentials_dir.clone()));
                }
            }
            Ok(LocalPlayerCommand::Disconnect) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.disconnect(true)
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Play) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.play()
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Pause) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.pause()
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Previous) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.prev()
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Next) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.next()
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Seek { position_ms }) => {
                if let Some(player) = active.as_ref() {
                    let position = u32::try_from(position_ms).unwrap_or(u32::MAX);
                    if let Err(error) = player.spirc.set_position_ms(position) {
                        playback_error(&events, error.to_string());
                    }
                }
            }
            Ok(LocalPlayerCommand::Volume { value_milli }) => {
                if let Some(player) = active.as_ref() {
                    let value = u32::from(value_milli.min(1000));
                    let volume =
                        u16::try_from((value * u32::from(u16::MAX)) / 1000).unwrap_or(u16::MAX);
                    if let Err(error) = player.spirc.set_volume(volume) {
                        playback_error(&events, error.to_string());
                    }
                }
            }
            Ok(LocalPlayerCommand::Shuffle { enabled }) => {
                if let Some(player) = active.as_ref()
                    && let Err(error) = player.spirc.shuffle(enabled)
                {
                    playback_error(&events, error.to_string());
                }
            }
            Ok(LocalPlayerCommand::Repeat { mode }) => {
                if let Some(player) = active.as_ref() {
                    let result = match mode {
                        0 => player
                            .spirc
                            .repeat_track(false)
                            .and_then(|()| player.spirc.repeat(false)),
                        1 => player
                            .spirc
                            .repeat_track(false)
                            .and_then(|()| player.spirc.repeat(true)),
                        2 => player
                            .spirc
                            .repeat(false)
                            .and_then(|()| player.spirc.repeat_track(true)),
                        _ => {
                            playback_error(&events, format!("invalid repeat mode {mode}"));
                            continue;
                        }
                    };
                    if let Err(error) = result {
                        playback_error(&events, error.to_string());
                    }
                }
            }
            Ok(LocalPlayerCommand::TokenUpdate { .. }) | Err(mpsc::TryRecvError::Empty) => {}
            Ok(LocalPlayerCommand::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => {
                cancel_connection(&mut connecting, &events);
                shutdown_active(active.take());
                break;
            }
        }

        if connecting.as_ref().is_some_and(tokio::task::JoinHandle::is_finished) {
            connection_pending.store(false, Ordering::Release);
            match connecting.take().expect("checked connection").await {
                Ok(Ok(player)) => active = Some(player),
                result => {
                    let error = match result {
                        Ok(Err(error)) => error,
                        Err(error) => format!("startup task failed: {error}"),
                        Ok(Ok(_)) => unreachable!(),
                    };
                    let _ = events.try_send(LocalPlayerEvent::Unavailable);
                    playback_error(&events, error);
                }
            }
        }

        if active.as_ref().is_some_and(|player| player.spirc_task.is_finished()) {
            let was_ready = active.as_ref().is_some_and(|player| player.ready_emitted);
            shutdown_active(active.take());
            let _ = events.try_send(LocalPlayerEvent::Unavailable);
            playback_error(
                &events,
                if was_ready {
                    "Spotify Connect session stopped unexpectedly after becoming ready"
                } else {
                    "Spotify Connect session stopped during startup before the device became ready"
                },
            );
        } else if let Some(player) = active.as_mut()
            && !player.ready_emitted
            && Instant::now() >= player.ready_at
        {
            player.ready_emitted = true;
            tracing::info!(
                device_id = %player.device_id,
                "librespot startup grace period completed; reporting local device ready"
            );
            let _ =
                events.try_send(LocalPlayerEvent::Ready { device_id: player.device_id.clone() });
        }

        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

// Setup is kept in dependency order so errors unwind the session and audio together.
#[allow(clippy::too_many_lines)]
async fn connect(credentials_dir: PathBuf) -> std::result::Result<ActivePlayer, String> {
    const REGISTRATION_GRACE_PERIOD: Duration = Duration::from_secs(3);

    let session_config =
        SessionConfig { device_id: random_device_id(), ..SessionConfig::default() };
    let device_id = DeviceId::parse(session_config.device_id.clone())
        .map_err(|error| format!("generated device id is invalid: {error}"))?;

    let cache = Cache::new(Some(credentials_dir.clone()), None, None, None)
        .map_err(|error| format!("could not open Librespot credential cache: {error}"))?;

    let credentials = if let Some(credentials) = cache.credentials() {
        tracing::info!(
            path = %credentials_dir.display(),
            "using cached Librespot credentials"
        );
        credentials
    } else {
        tracing::info!(
            path = %credentials_dir.display(),
            "no cached Librespot credentials; starting one-time Spotify authorization"
        );
        oauth_credentials(session_config.client_id.clone()).await?
    };

    let session = Session::new(session_config, Some(cache));

    tracing::info!(device_id = %device_id, "initializing librespot audio and Spotify session");

    let mixer_builder = mixer::find(None)
        .ok_or_else(|| "librespot soft-volume mixer is unavailable".to_string())?;
    let mixer = mixer_builder(MixerConfig::default())
        .map_err(|error| format!("could not initialize librespot mixer: {error}"))?;

    let preferred_backend = preferred_audio_backend();
    if let Some(name) = preferred_backend.as_deref() {
        tracing::info!(backend = name, "selecting librespot audio backend");
    }
    let sink_builder = preferred_backend
        .clone()
        .and_then(|name| audio_backend::find(Some(name)))
        .or_else(|| audio_backend::find(None))
        .ok_or_else(|| "librespot audio backend is unavailable".to_string())?;
    let player =
        Player::new(PlayerConfig::default(), session.clone(), mixer.get_soft_volume(), move || {
            sink_builder(None, AudioFormat::default())
        });

    let connect_config = ConnectConfig {
        name: "lspotify".into(),
        device_type: DeviceType::Computer,
        ..ConnectConfig::default()
    };
    let (spirc, spirc_task) =
        Spirc::new(connect_config, session.clone(), credentials, Arc::clone(&player), mixer)
            .await
            .map_err(|error| {
                format!("Spotify/Librespot authentication or SPIRC setup failed: {error}")
            })?;

    // NOTE: activate may claim playback ownership before an explicit user transfer.
    // Keep this startup behavior until it can be verified with a live Spotify account.
    spirc
        .activate()
        .map_err(|error| format!("could not activate the Librespot Connect device: {error}"))?;

    tracing::info!(
        device_id = %device_id,
        "librespot SPIRC initialized and activation queued"
    );

    let spirc_task = tokio::spawn(spirc_task);

    Ok(ActivePlayer {
        session,
        spirc,
        _player: player,
        spirc_task,
        device_id,
        ready_at: Instant::now() + REGISTRATION_GRACE_PERIOD,
        ready_emitted: false,
    })
}

async fn connection_with_timeout<T>(
    future: impl std::future::Future<Output = std::result::Result<T, String>>,
    timeout: Duration,
) -> std::result::Result<T, String> {
    tokio::time::timeout(timeout, future).await.unwrap_or_else(|_| {
        Err(format!(
            "startup authorization timed out after {} seconds; select This computer to retry",
            timeout.as_secs()
        ))
    })
}

type ConnectionTask = tokio::task::JoinHandle<std::result::Result<ActivePlayer, String>>;

fn cancel_connection(
    task: &mut Option<ConnectionTask>,
    events: &mpsc::SyncSender<LocalPlayerEvent>,
) {
    if let Some(task) = task.take() {
        task.abort();
        playback_error(events, "startup canceled by shutdown; local playback can be restarted");
    }
}

async fn oauth_credentials(client_id: String) -> std::result::Result<Credentials, String> {
    type OAuthResult = std::result::Result<(Credentials, Instant), String>;
    // Librespot's blocking callback listener cannot be canceled. Reuse its single pending
    // attempt after timeout/backend replacement, avoiding leaked listeners and port conflicts.
    // NOTE: librespot-oauth 0.8 keeps set_auth_url private and exposes neither the
    // PKCE URL nor listener cancellation. A retry must reuse this listener; it cannot
    // reopen/log the original URL through the public API. Reuse the original browser tab.
    static PENDING: OnceLock<Mutex<Option<mpsc::Receiver<OAuthResult>>>> = OnceLock::new();
    loop {
        {
            let mut pending = PENDING
                .get_or_init(|| Mutex::new(None))
                .lock()
                .map_err(|_| "OAuth worker lock was poisoned".to_string())?;
            if pending.is_none() {
                let (sender, receiver) = mpsc::channel();
                let client_id = client_id.clone();
                thread::Builder::new()
                    .name("lspotify-oauth".into())
                    .spawn(move || {
                        let result = OAuthClientBuilder::new(
                            &client_id,
                            "http://127.0.0.1:8898/login",
                            vec!["streaming"],
                        )
                        .open_in_browser()
                        .build()
                        .map_err(|error| format!("could not prepare OAuth: {error}"))
                        .and_then(|client| {
                            client
                                .get_access_token()
                                .map(|token| {
                                    (
                                        Credentials::with_access_token(token.access_token),
                                        token.expires_at,
                                    )
                                })
                                .map_err(|error| format!("OAuth failed: {error}"))
                        });
                        let _ = sender.send(result);
                    })
                    .map_err(|error| format!("could not start OAuth worker: {error}"))?;
                *pending = Some(receiver);
            }
            match pending.as_ref().expect("initialized OAuth").try_recv() {
                Ok(result) => {
                    *pending = None;
                    return result.and_then(|(credentials, expires_at)| {
                        if expires_at <= Instant::now() {
                            Err("OAuth token expired while waiting; select This computer to retry"
                                .into())
                        } else {
                            Ok(credentials)
                        }
                    });
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    *pending = None;
                    return Err("OAuth worker stopped; select This computer to retry".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

impl Drop for ActivePlayer {
    fn drop(&mut self) {
        let _ = self.spirc.shutdown();
        self.session.shutdown();
        self.spirc_task.abort();
    }
}

fn shutdown_active(active: Option<ActivePlayer>) {
    drop(active);
}

fn preferred_audio_backend() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let pulse_socket_available = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .is_some_and(|runtime| runtime.join("pulse").join("native").exists());
        if std::env::var_os("PULSE_SERVER").is_some()
            || std::env::var_os("WSL_DISTRO_NAME").is_some()
            || pulse_socket_available
        {
            return Some("pulseaudio".into());
        }
    }
    None
}

fn playback_error(events: &mpsc::SyncSender<LocalPlayerEvent>, message: impl Into<String>) {
    let _ =
        events.try_send(LocalPlayerEvent::PlaybackError(format!("Librespot: {}", message.into())));
}

fn random_device_id() -> String {
    let bytes: [u8; 20] = rand::rng().random();
    bytes.iter().fold(String::with_capacity(40), |mut id, byte| {
        let _ = write!(id, "{byte:02x}");
        id
    })
}

fn poisoned() -> AppError {
    AppError::new(ErrorKind::Unexpected, "librespot player lock was poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_command_queue_returns_error_and_duplicate_connect_is_not_queued() {
        let (commands, receiver) = mpsc::sync_channel(1);
        let (_, events) = mpsc::sync_channel(1);
        let player = LibrespotLocalPlayer {
            commands,
            events: Mutex::new(events),
            thread: Mutex::new(None),
            stopping: Arc::new(AtomicBool::new(false)),
            connecting: Arc::new(AtomicBool::new(false)),
        };
        player.send(LocalPlayerCommand::Connect).unwrap();
        player.send(LocalPlayerCommand::Connect).unwrap();
        assert!(player.send(LocalPlayerCommand::Pause).is_err());
        assert!(matches!(receiver.try_recv(), Ok(LocalPlayerCommand::Connect)));
        assert!(receiver.try_recv().is_err());
        player.send(LocalPlayerCommand::Shutdown).unwrap();
        assert!(player.stopping.load(Ordering::Acquire));
    }

    #[test]
    fn pending_connection_does_not_block_shutdown_or_queue_connects() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let (commands, receiver) = mpsc::sync_channel(4);
            let (events, event_receiver) = mpsc::sync_channel(4);
            commands.send(LocalPlayerCommand::Connect).unwrap();
            commands.send(LocalPlayerCommand::Connect).unwrap();
            commands.send(LocalPlayerCommand::Shutdown).unwrap();
            let mut attempts = 0;
            run_async(receiver, events, PathBuf::new(), Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(true)), |_| {
                attempts += 1;
                tokio::spawn(std::future::pending())
            }).await;
            assert_eq!(attempts, 1);
            assert!(event_receiver.try_iter().any(|event| matches!(event, LocalPlayerEvent::PlaybackError(message) if message.contains("canceled"))));
        });
    }

    #[test]
    fn stalled_connection_times_out_and_can_be_retried() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let result =
                connection_with_timeout::<()>(std::future::pending(), Duration::from_millis(1))
                    .await;
            assert!(result.unwrap_err().contains("timed out"));
            assert_eq!(
                connection_with_timeout(async { Ok(42) }, Duration::from_secs(1)).await.unwrap(),
                42
            );
        });
    }

    #[test]
    fn generated_device_ids_fit_spotify_identifier_rules() {
        let id = random_device_id();
        assert_eq!(id.len(), 40);
        assert!(DeviceId::parse(id).is_ok());
    }
}
