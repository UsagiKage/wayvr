use super::kde_capture::KdeCapture;
use crate::subsystem::input_lease::InputPermit;
use dbus::{
    blocking::Connection,
    channel::{MatchingReceiver, Sender},
    message::MatchRule,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const FRESHNESS: Duration = Duration::from_millis(160);
#[derive(Clone, Debug, Deserialize)]
pub struct Snapshot {
    token: String,
    pub uuid: String,
    pub visible: bool,
    pub active: bool,
    pub transients: bool,
    pub geometry: [f64; 4],
    pub scale: f64,
    pub generation: u64,
    pub probe_seq: u64,
    pub hit_uuid: String,
    pub probe: Option<[f64; 2]>,
}
#[derive(Default, Serialize)]
struct Commands {
    activate: bool,
    probe: Option<[f64; 2]>,
    probe_seq: u64,
}
pub struct Shared {
    pub uuid: String,
    snapshot: Mutex<Option<(Snapshot, Instant)>>,
    commands: Mutex<Commands>,
    pub alive: AtomicBool,
    pub armed: AtomicBool,
    pub accepted: AtomicBool,
    pub capture_generation: AtomicU64,
}
impl Shared {
    pub(super) fn disconnected(uuid: String) -> Arc<Self> {
        Arc::new(Self {
            uuid,
            snapshot: Mutex::new(None),
            commands: Mutex::new(Commands::default()),
            alive: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            accepted: AtomicBool::new(false),
            capture_generation: AtomicU64::new(u64::MAX),
        })
    }
    #[cfg(feature = "experimental-kde-driver")]
    pub(super) fn simulate_snapshot(&self, snapshot: Snapshot, age: Duration) {
        *self.snapshot.lock().unwrap() = Some((snapshot, Instant::now() - age));
    }
    #[cfg(feature = "experimental-kde-driver")]
    pub(super) fn simulation_commands(&self) -> (bool, Option<[f64; 2]>, u64) {
        let mut commands = self.commands.lock().unwrap();
        let activate = std::mem::take(&mut commands.activate);
        (activate, commands.probe, commands.probe_seq)
    }
    pub fn snapshot(&self) -> Option<Snapshot> {
        let snapshot = self.snapshot.lock().ok()?;
        let (s, at) = snapshot.as_ref()?;
        (at.elapsed() <= FRESHNESS && self.alive.load(Ordering::Acquire)).then(|| s.clone())
    }
    pub fn activate(&self) {
        self.commands.lock().unwrap().activate = true;
    }
    pub fn probe(&self, point: [f64; 2]) -> u64 {
        let mut command = self.commands.lock().unwrap();
        if command.probe != Some(point) {
            command.probe = Some(point);
            command.probe_seq = command.probe_seq.wrapping_add(1);
        }
        command.probe_seq
    }
    pub fn target_valid(&self) -> bool {
        self.snapshot().is_some_and(|s| {
            s.uuid == self.uuid
                && s.visible
                && s.active
                && !s.transients
                && valid_geometry(&s)
                && self.capture_generation.load(Ordering::Acquire) == s.generation
        })
    }
}
impl InputPermit for Shared {
    fn valid(&self) -> bool {
        self.armed.load(Ordering::Acquire)
            && self.accepted.load(Ordering::Acquire)
            && self.target_valid()
    }
    fn revoke(&self) {
        self.armed.store(false, Ordering::Release);
    }
}
pub fn valid_geometry(s: &Snapshot) -> bool {
    s.geometry.iter().all(|v| v.is_finite())
        && s.geometry[2] > 1.
        && s.geometry[3] > 1.
        && s.scale.is_finite()
        && s.scale > 0.
        && s.scale <= 8.
}
pub fn map_point(s: &Snapshot, uv: [f64; 2]) -> Option<[f64; 2]> {
    if !valid_geometry(s) || uv.iter().any(|v| !v.is_finite() || !(0.0..1.0).contains(v)) {
        return None;
    }
    Some([
        s.geometry[0] + uv[0] * s.geometry[2],
        s.geometry[1] + uv[1] * s.geometry[3],
    ])
}

pub struct Session {
    pub shared: Arc<Shared>,
    pub node: std::sync::mpsc::Receiver<Result<u32, String>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
impl Session {
    pub fn start(uuid: &str) -> anyhow::Result<Self> {
        let uuid = uuid::Uuid::parse_str(uuid)?.braced().to_string();
        let shared = Arc::new(Shared {
            uuid,
            snapshot: Mutex::new(None),
            commands: Mutex::new(Commands::default()),
            alive: AtomicBool::new(false),
            armed: AtomicBool::new(false),
            accepted: AtomicBool::new(false),
            capture_generation: AtomicU64::new(u64::MAX),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let thread_shared = shared.clone();
        let thread_stop = stop.clone();
        let worker = thread::spawn(move || {
            let result = run(&thread_shared, &thread_stop, &sender);
            thread_shared.alive.store(false, Ordering::Release);
            thread_shared.revoke();
            if let Err(error) = result {
                let _ = sender.try_send(Err(error.to_string()));
                log::error!("Experimental KDE mirror stopped: {error:#}");
            }
        });
        Ok(Self {
            shared,
            stop,
            worker: Some(worker),
            node: receiver,
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.shared.revoke();
        self.shared.alive.store(false, Ordering::Release);
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct ScriptGuard {
    connection: Connection,
    name: String,
    directory: std::path::PathBuf,
}
impl Drop for ScriptGuard {
    fn drop(&mut self) {
        let _: Result<(bool,), _> = self
            .connection
            .with_proxy("org.kde.KWin", "/Scripting", Duration::from_millis(500))
            .method_call("org.kde.kwin.Scripting", "unloadScript", (&self.name,));
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn run(
    shared: &Arc<Shared>,
    stop: &AtomicBool,
    sender: &std::sync::mpsc::SyncSender<Result<u32, String>>,
) -> anyhow::Result<()> {
    let conn = Connection::new_session()?;
    let (kwin_owner,): (String,) = conn
        .with_proxy(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            Duration::from_secs(1),
        )
        .method_call("org.freedesktop.DBus", "GetNameOwner", ("org.kde.KWin",))?;
    let (info,): (dbus::arg::PropMap,) = conn
        .with_proxy("org.kde.KWin", "/KWin", Duration::from_secs(1))
        .method_call("org.kde.KWin", "getWindowInfo", (&shared.uuid,))?;
    anyhow::ensure!(
        info.get("uuid").and_then(|v| v.0.as_str()) == Some(shared.uuid.as_str()),
        "Unknown KWin window UUID"
    );
    anyhow::ensure!(
        info.get("excludeFromCapture").and_then(|v| v.0.as_i64()) == Some(0),
        "KWin forbids capture, or capture permission is unknown"
    );
    let (mut capture, node) = KdeCapture::start(&shared.uuid)?;
    anyhow::ensure!(
        !stop.load(Ordering::Acquire),
        "Mirror closed during initialization"
    );
    let token = uuid::Uuid::new_v4().to_string();
    let destination = conn.unique_name().to_string();
    let callback_shared = shared.clone();
    let callback_token = token.clone();
    conn.start_receive(
        MatchRule::new_method_call()
            .with_interface("org.dqi.WayVRMirror")
            .with_member("Exchange"),
        Box::new(move |message, connection| {
            let trusted = message.sender().is_some_and(|s| &*s == kwin_owner.as_str());
            let snapshot = message
                .read1::<String>()
                .ok()
                .filter(|s| s.len() < 8192)
                .and_then(|s| serde_json::from_str::<Snapshot>(&s).ok());
            let valid = trusted
                && snapshot
                    .as_ref()
                    .is_some_and(|s| s.token == callback_token && s.uuid == callback_shared.uuid);
            if valid {
                *callback_shared.snapshot.lock().unwrap() = snapshot.map(|s| (s, Instant::now()));
                let mut commands = callback_shared.commands.lock().unwrap();
                let response = serde_json::to_string(&*commands).unwrap_or_default();
                commands.activate = false;
                let _ = connection.send(message.method_return().append1(response));
            } else {
                let _ = connection.send(message.method_return().append1("{}"));
            }
            true
        }),
    );
    let name = format!("dqi-wayvr-mirror-{}", uuid::Uuid::new_v4());
    let directory = std::env::temp_dir().join(&name);
    fs::create_dir(&directory)?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    let script = directory.join("bridge.js");
    let source = include_str!("kde_bridge.js")
        .replace("__UUID__", &serde_json::to_string(&shared.uuid)?)
        .replace("__DESTINATION__", &serde_json::to_string(&destination)?)
        .replace("__TOKEN__", &serde_json::to_string(&token)?);
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&script)?
        .write_all(source.as_bytes())?;
    let guard = ScriptGuard {
        connection: conn,
        name,
        directory,
    };
    let (id,): (i32,) = guard
        .connection
        .with_proxy("org.kde.KWin", "/Scripting", Duration::from_secs(1))
        .method_call(
            "org.kde.kwin.Scripting",
            "loadScript",
            (script.to_string_lossy().as_ref(), &guard.name),
        )?;
    anyhow::ensure!(id >= 0, "KWin refused temporary script");
    let _: () = guard
        .connection
        .with_proxy(
            "org.kde.KWin",
            format!("/Scripting/Script{id}"),
            Duration::from_secs(1),
        )
        .method_call("org.kde.kwin.Script", "run", ())?;
    shared.alive.store(true, Ordering::Release);
    log::info!("Loaded temporary KWin script: {}", guard.name);
    sender.try_send(Ok(node))?;
    while !stop.load(Ordering::Acquire) {
        capture.pump(0)?;
        guard.connection.process(Duration::from_millis(10))?;
        if shared.accepted.load(Ordering::Acquire)
            && let Some(snapshot) = shared.snapshot()
            && (!snapshot.visible || !snapshot.active || snapshot.transients)
        {
            shared.revoke();
        }
        // A stalled KWin helper cannot silently regain a revoked interaction lease.
        if shared
            .snapshot
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > FRESHNESS)
        {
            shared.revoke();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn permit() -> Arc<Shared> {
        let s = Snapshot {
            token: String::new(),
            uuid: "test-window".into(),
            visible: true,
            active: true,
            transients: false,
            geometry: [0., 0., 800., 600.],
            scale: 1.,
            generation: 2,
            probe_seq: 1,
            probe: Some([100., 100.]),
            hit_uuid: "test-window".into(),
        };
        Arc::new(Shared {
            uuid: s.uuid.clone(),
            snapshot: Mutex::new(Some((s, Instant::now()))),
            commands: Mutex::new(Commands::default()),
            alive: AtomicBool::new(true),
            armed: AtomicBool::new(true),
            accepted: AtomicBool::new(true),
            capture_generation: AtomicU64::new(2),
        })
    }
    #[test]
    fn refuses_stale_metadata_capture_loss_wrong_identity_and_geometry_epoch_mismatch() {
        let permit = permit();
        assert!(permit.valid());
        permit.snapshot.lock().unwrap().as_mut().unwrap().1 =
            Instant::now() - FRESHNESS - Duration::from_millis(1);
        assert!(!permit.valid());
        permit.snapshot.lock().unwrap().as_mut().unwrap().1 = Instant::now();
        permit.alive.store(false, Ordering::Release);
        assert!(!permit.valid());
        permit.alive.store(true, Ordering::Release);
        permit.capture_generation.store(1, Ordering::Release);
        assert!(!permit.valid());
        permit.capture_generation.store(2, Ordering::Release);
        permit.snapshot.lock().unwrap().as_mut().unwrap().0.uuid = "other-window".into();
        assert!(!permit.valid());
    }
    #[test]
    fn refuses_focus_loss_minimization_and_popup_geometry() {
        let permit = permit();
        permit.snapshot.lock().unwrap().as_mut().unwrap().0.active = false;
        assert!(!permit.valid());
        permit.snapshot.lock().unwrap().as_mut().unwrap().0.active = true;
        permit.snapshot.lock().unwrap().as_mut().unwrap().0.visible = false;
        assert!(!permit.valid());
        permit.snapshot.lock().unwrap().as_mut().unwrap().0.visible = true;
        permit
            .snapshot
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .0
            .transients = true;
        assert!(!permit.valid());
    }
    #[test]
    fn activation_requires_acceptance_and_revocation_cannot_resume_on_focus_return() {
        let permit = permit();
        permit.accepted.store(false, Ordering::Release);
        assert!(permit.target_valid());
        assert!(!permit.valid());
        permit.accepted.store(true, Ordering::Release);
        assert!(permit.valid());
        permit.revoke();
        assert!(permit.target_valid());
        assert!(!permit.valid());
    }
    #[test]
    fn logical_mapping_handles_negative_monitor_origins_and_rejects_invalid_coordinates() {
        let snapshot = Snapshot {
            token: String::new(),
            uuid: String::new(),
            visible: true,
            active: true,
            transients: false,
            geometry: [-1200., 100., 800., 600.],
            scale: 1.5,
            generation: 1,
            probe_seq: 1,
            hit_uuid: String::new(),
            probe: None,
        };
        assert_eq!(map_point(&snapshot, [0.5, 0.5]), Some([-800., 400.]));
        assert_eq!(map_point(&snapshot, [1., 0.5]), None);
        assert_eq!(map_point(&snapshot, [f64::NAN, 0.5]), None);
        let mut bad = snapshot;
        bad.scale = 0.;
        assert_eq!(map_point(&bad, [0.5, 0.5]), None);
    }
}
