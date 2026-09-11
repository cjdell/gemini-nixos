//! Async cache for the [`gemdata::DataProvider`].
//!
//! The real provider shells out to `nmcli`/`bluetoothctl`/`wpctl`; a
//! full snapshot is ~1–2 s on the Gemini. The settings panel used to
//! call that synchronously on the compositor thread — on open and again
//! every 3 s — which froze the whole UI (taps, launcher, buttons) for
//! seconds at a time (reported on glass 2026-09-11).
//!
//! `CachedData` answers every read from memory (instant, so the UI
//! thread never blocks). Mutations and refreshes go to a worker thread
//! that owns the real provider, with two coalescing rules that matter:
//!
//!  - **Refreshes are coalesced** (`refresh_pending`): requesting a
//!    refresh while one is already pending is a no-op, so a slow snapshot
//!    can never build a backlog.
//!  - **Mutations are coalesced by kind** (`pending`, last wins): a
//!    brightness/volume slider drag produces one mutation per frame, but
//!    only the latest value is ever applied.
//!
//! The worker runs pending mutations *before* a snapshot, so a fast
//! mutation (a devmem backlight write) is never stuck behind a slow
//! `nmcli` call for long. Fast, locally-represented mutations
//! (brightness/volume/mute) do not request a snapshot at all — the panel
//! already shows the value it just set. Without these, a slider drag
//! queued a ~2 s snapshot per frame and taps took 30 s+ to be handled
//! (reported on glass 2026-09-11).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};

use gemdata::{AudioState, BtState, DataProvider, Result as DataResult, ShellStatus, WifiState};

type Mutation = Box<dyn FnOnce(&dyn DataProvider) -> DataResult<()> + Send>;

enum Cmd {
    /// Run whatever is pending (mutations, then a snapshot if requested).
    Poke,
}

#[derive(Clone, Default)]
struct Snapshot {
    status: ShellStatus,
    wifi: WifiState,
    bt: BtState,
    audio: AudioState,
}

pub struct CachedData {
    /// Shared with the worker thread (which publishes new snapshots).
    cache: Arc<Mutex<Snapshot>>,
    tx: mpsc::Sender<Cmd>,
    /// Pending mutations, coalesced by kind (last write wins).
    pending: Arc<Mutex<Vec<(&'static str, Mutation)>>>,
    /// A snapshot has been requested; the worker clears it when it runs.
    refresh_pending: Arc<AtomicBool>,
}

impl CachedData {
    /// Spawn the worker thread. The returned receiver fires once per
    /// published snapshot (initial read included) so the compositor can
    /// mark the panel dirty.
    pub fn new(inner: Arc<dyn DataProvider>) -> (Arc<Self>, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel::<Cmd>();
        let (notify_tx, notify_rx) = mpsc::channel::<()>();
        let cache = Arc::new(Mutex::new(Snapshot::default()));
        let pending: Arc<Mutex<Vec<(&'static str, Mutation)>>> = Arc::new(Mutex::new(Vec::new()));
        // initial snapshot requested
        let refresh_pending = Arc::new(AtomicBool::new(true));
        let worker_cache = cache.clone();
        let worker_pending = pending.clone();
        let worker_refresh = refresh_pending.clone();
        std::thread::Builder::new()
            .name("gemshell-data".into())
            .spawn(move || loop {
                // Block until asked to do something.
                if rx.recv().is_err() {
                    break; // CachedData dropped
                }
                // Absorb any further immediate requests so a burst of
                // pokes becomes one round of work.
                while rx.try_recv().is_ok() {}
                // Run the pending mutations first: they are cheap, and a
                // fast one must not wait behind the snapshot below.
                let batch: Vec<(&'static str, Mutation)> = {
                    let mut p = worker_pending.lock().unwrap();
                    std::mem::take(&mut *p)
                };
                for (_kind, f) in batch {
                    if let Err(e) = f(&*inner) {
                        log::warn!("data mutation failed: {e}");
                    }
                }
                // Publish at most one snapshot per wake.
                if worker_refresh.swap(false, Ordering::SeqCst) {
                    let snap = Snapshot {
                        status: inner.status(),
                        wifi: inner.wifi(),
                        bt: inner.bluetooth(),
                        audio: inner.audio(),
                    };
                    if let Ok(mut c) = worker_cache.lock() {
                        *c = snap;
                    }
                    let _ = notify_tx.send(());
                }
            })
            .ok();
        (
            Arc::new(CachedData { cache, tx, pending, refresh_pending }),
            notify_rx,
        )
    }

    /// Request a snapshot (non-blocking, coalesced: at most one runs at a
    /// time). The result arrives via the notify receiver.
    pub fn refresh(&self) {
        self.refresh_pending.store(true, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Poke);
    }

    /// Queue a mutation, coalesced by `kind` (last value wins). Set
    /// `need_snapshot` when the UI needs to read back system state (Wi-Fi
    /// connect, sink change); leave it false for locally-represented
    /// values (brightness/volume/mute) so a drag never triggers a
    /// snapshot per frame.
    fn mutate<F>(&self, kind: &'static str, need_snapshot: bool, f: F) -> DataResult<()>
    where
        F: FnOnce(&dyn DataProvider) -> DataResult<()> + Send + 'static,
    {
        {
            let mut p = self.pending.lock().unwrap();
            p.retain(|(k, _)| *k != kind);
            p.push((kind, Box::new(f)));
        }
        if need_snapshot {
            self.refresh_pending.store(true, Ordering::SeqCst);
        }
        let _ = self.tx.send(Cmd::Poke);
        Ok(())
    }
}

impl DataProvider for CachedData {
    fn status(&self) -> ShellStatus {
        self.cache.lock().map(|c| c.status).unwrap_or_default()
    }

    fn wifi(&self) -> WifiState {
        self.cache.lock().map(|c| c.wifi.clone()).unwrap_or_default()
    }

    fn set_wifi_enabled(&self, enabled: bool) -> DataResult<()> {
        self.mutate("wifi_enabled", true, move |d| d.set_wifi_enabled(enabled))
    }

    fn scan_wifi(&self) -> DataResult<()> {
        self.mutate("wifi_scan", true, |d| d.scan_wifi())
    }

    fn connect_wifi(&self, ssid: &str, password: Option<&str>) -> DataResult<()> {
        let ssid = ssid.to_string();
        let password = password.map(str::to_string);
        self.mutate("wifi_connect", true, move |d| {
            d.connect_wifi(&ssid, password.as_deref())
        })
    }

    fn disconnect_wifi(&self) -> DataResult<()> {
        self.mutate("wifi_disconnect", true, |d| d.disconnect_wifi())
    }

    fn bluetooth(&self) -> BtState {
        self.cache.lock().map(|c| c.bt.clone()).unwrap_or_default()
    }

    fn set_bluetooth_enabled(&self, enabled: bool) -> DataResult<()> {
        self.mutate("bt_enabled", true, move |d| d.set_bluetooth_enabled(enabled))
    }

    fn connect_bluetooth(&self, mac: &str) -> DataResult<()> {
        let mac = mac.to_string();
        self.mutate("bt_connect", true, move |d| d.connect_bluetooth(&mac))
    }

    fn scan_bluetooth(&self) -> DataResult<()> {
        self.mutate("bt_scan", true, |d| d.scan_bluetooth())
    }

    fn audio(&self) -> AudioState {
        self.cache.lock().map(|c| c.audio.clone()).unwrap_or_default()
    }

    fn set_default_sink(&self, id: &str) -> DataResult<()> {
        let id = id.to_string();
        self.mutate("sink", true, move |d| d.set_default_sink(&id))
    }

    fn set_volume(&self, volume: f32) -> DataResult<()> {
        // Locally represented: no snapshot needed (and a drag must not
        // queue one per frame).
        self.mutate("volume", false, move |d| d.set_volume(volume))
    }

    fn set_muted(&self, muted: bool) -> DataResult<()> {
        self.mutate("muted", false, move |d| d.set_muted(muted))
    }

    fn set_brightness(&self, percent: i32) -> DataResult<()> {
        self.mutate("brightness", false, move |d| d.set_brightness(percent))
    }
}
