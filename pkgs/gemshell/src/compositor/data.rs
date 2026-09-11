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
    /// The provider itself, for mutations that are cheap enough to run on
    /// the caller's thread. `set_brightness` is a devmem + sysfs write
    /// (microseconds), so running it inline makes the Settings slider
    /// track the finger exactly instead of waiting for the worker thread
    /// (which may be mid-`nmcli`). Report: "brightness slider extremely
    /// unresponsive" (2026-09-11).
    direct: Arc<dyn DataProvider>,
}

/// Run every pending mutation (they are cheap; the snapshot below is not).
fn run_pending(
    pending: &Arc<Mutex<Vec<(&'static str, Mutation)>>>,
    inner: &dyn DataProvider,
) {
    let batch: Vec<(&'static str, Mutation)> = {
        let mut p = pending.lock().unwrap();
        std::mem::take(&mut *p)
    };
    for (_kind, f) in batch {
        if let Err(e) = f(inner) {
            log::warn!("data mutation failed: {e}");
        }
    }
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
        let worker_inner = inner.clone();
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
                run_pending(&worker_pending, &*worker_inner);
                // Publish at most one snapshot per wake. Drain mutations
                // between the slow provider calls too, so a mutation that
                // arrives mid-snapshot waits at most one call (~250-750 ms)
                // rather than the whole ~1-2 s snapshot.
                if worker_refresh.swap(false, Ordering::SeqCst) {
                    let status = worker_inner.status();
                    run_pending(&worker_pending, &*worker_inner);
                    let wifi = worker_inner.wifi();
                    run_pending(&worker_pending, &*worker_inner);
                    let bt = worker_inner.bluetooth();
                    run_pending(&worker_pending, &*worker_inner);
                    let audio = worker_inner.audio();
                    if let Ok(mut c) = worker_cache.lock() {
                        *c = Snapshot { status, wifi, bt, audio };
                    }
                    let _ = notify_tx.send(());
                }
            })
            .ok();
        (
            Arc::new(CachedData {
                cache,
                tx,
                pending,
                refresh_pending,
                direct: inner,
            }),
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
        // Direct: a devmem + sysfs write, cheap enough for the UI thread,
        // and it must track a slider drag without queueing behind a slow
        // snapshot. (Queued brightness mutations would also race an older
        // value onto the panel.)
        self.direct.set_brightness(percent)
    }
}
