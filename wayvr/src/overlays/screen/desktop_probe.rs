//! Native, non-injecting preflight. No device/input provider or VR startup.
use super::kde_session::Session;
use dbus::blocking::Connection;
use serde_json::json;
use std::time::{Duration, Instant};
use wlx_capture::{
    WlxCapture,
    frame::{FrameFormat, Transform, WlxFrame},
    pipewire::capture::PipewireCapture,
};

fn frame_extent(_: &(), frame: WlxFrame) -> Option<FrameFormat> {
    match frame {
        WlxFrame::Dmabuf(frame) => Some(frame.format),
        WlxFrame::MemFd(frame) => Some(frame.format),
        WlxFrame::MemPtr(frame) => Some(frame.format),
        WlxFrame::Implicit(_) => None,
    }
}

fn process_id(value: &dyn dbus::arg::RefArg) -> Option<u32> {
    value
        .as_u64()
        .or_else(|| value.as_i64().and_then(|v| u64::try_from(v).ok()))
        .and_then(|v| u32::try_from(v).ok())
        .filter(|v| *v > 0)
}

pub fn run(uuid: &str, pid: u32, title: &str) -> anyhow::Result<()> {
    let uuid = uuid::Uuid::parse_str(uuid)?.braced().to_string();
    anyhow::ensure!(
        title.starts_with("wayvr-disposable-") && title.ends_with(".txt"),
        "Only a specifically named disposable test document is accepted"
    );
    let connection = Connection::new_session()?;
    let (info,): (dbus::arg::PropMap,) = connection
        .with_proxy("org.kde.KWin", "/KWin", Duration::from_secs(1))
        .method_call("org.kde.KWin", "getWindowInfo", (&uuid,))?;
    let text = |key: &str| info.get(key).and_then(|v| v.0.as_str());
    let actual_pid = info.get("pid").and_then(|v| process_id(&*v.0));
    println!(
        "{}",
        json!({"stage":"identity", "uuid":text("uuid"),
        "application":text("resourceClass"), "caption":text("caption"),
        "pid":actual_pid, "real_input":false, "vr_runtime":false})
    );
    anyhow::ensure!(
        text("uuid") == Some(uuid.as_str()) && actual_pid == Some(pid),
        "Selected target UUID/PID is not the expected disposable process"
    );
    anyhow::ensure!(
        matches!(text("resourceClass"), Some("org.kde.kwrite" | "kwrite")),
        "Target is not KWrite"
    );
    anyhow::ensure!(
        text("caption").is_some_and(|c| c.contains(title)),
        "Target is not the explicitly selected disposable document"
    );
    let session = Session::start(&uuid)?;
    let node = session
        .node
        .recv_timeout(Duration::from_secs(8))?
        .map_err(anyhow::Error::msg)?;
    println!(
        "{}",
        json!({"stage":"capture_authorized", "node":node, "real_input":false})
    );
    let mut capture =
        PipewireCapture::<FrameFormat>::new("WayVR disposable KWrite preflight".into(), node);
    capture.init(&[], (), frame_extent);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut extent = None;
    while Instant::now() < deadline {
        if let Some(format) =
            <PipewireCapture<FrameFormat> as WlxCapture<(), FrameFormat>>::receive(&mut capture)
        {
            anyhow::ensure!(
                matches!(format.transform, Transform::Normal | Transform::Undefined),
                "Unknown capture orientation; input remains disabled"
            );
            extent = Some([format.width, format.height]);
        }
        if let (Some(extent), Some(snapshot)) = (extent, session.shared.snapshot()) {
            anyhow::ensure!(
                super::kde_session::valid_geometry(&snapshot)
                    && snapshot.visible
                    && !snapshot.transients,
                "Target geometry/visibility is uncertain; input remains disabled"
            );
            anyhow::ensure!(
                extent
                    .into_iter()
                    .zip(snapshot.geometry[2..].iter())
                    .all(
                        |(actual, logical)| (f64::from(actual) - logical * snapshot.scale).abs()
                            <= 1.
                    ),
                "Actual capture extent disagrees with KWin geometry/scale; input remains disabled"
            );
            println!(
                "{}",
                json!({"stage":"native_preflight_passed", "uuid":uuid,
                "geometry":snapshot.geometry, "scale":snapshot.scale, "capture_extent":extent,
                "generation":snapshot.generation, "real_input":false, "vr_runtime":false})
            );
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    anyhow::bail!("No matching fresh capture and KWin telemetry; input remains disabled")
}

#[cfg(test)]
mod tests {
    use super::process_id;
    #[test]
    fn accepts_signed_kde_pid_without_accepting_invalid_values() {
        assert_eq!(process_id(&3763140_i32), Some(3763140));
        assert_eq!(process_id(&3763140_i64), Some(3763140));
        assert_eq!(process_id(&3763140_u32), Some(3763140));
        assert_eq!(process_id(&(-1_i32)), None);
        assert_eq!(process_id(&0_u32), None);
        assert_eq!(process_id(&u64::MAX), None);
        assert_eq!(process_id(&String::from("3763140")), None);
    }
}
