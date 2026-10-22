//! One explicitly selected existing KWin window. Experimental and fail-closed.
use super::{
    backend::{CaptureType, ScreenBackend},
    capture::{MainThreadWlxCapture, new_wlx_capture},
    kde_session::{Session, Snapshot, map_point},
};
use crate::{
    backend::input::{HoverResult, PointerHit, PointerMode},
    overlays::toast::Toast,
    state::AppState,
    subsystem::{
        hid::{MOUSE_LEFT, MOUSE_MIDDLE, MOUSE_RIGHT, WheelDelta},
        input::InputFocus,
        input_lease::InputPermit,
    },
    windowing::{
        backend::{FrameMeta, OverlayBackend, OverlayEventData, RenderResources, ShouldRender},
        window::{OverlayCategory, OverlayWindowConfig},
    },
};
use glam::{Affine2, Affine3A, DVec2, Quat, Vec3, vec3};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use wgui::i18n::Translation;
use wlx_capture::{WlxCapture, pipewire::capture::PipewireCapture};
use wlx_common::{
    overlays::{BackendAttrib, BackendAttribValue, ToastTopic},
    windowing::OverlayWindowState,
};

// A single experimental target keeps input ownership unambiguous in this milestone.
static IN_USE: AtomicBool = AtomicBool::new(false);
struct Reservation;
impl Drop for Reservation {
    fn drop(&mut self) {
        IN_USE.store(false, Ordering::Release);
    }
}

pub fn new_mirror(
    name: Arc<str>,
    uuid: &str,
    app: &mut AppState,
) -> anyhow::Result<OverlayWindowConfig> {
    anyhow::ensure!(
        !IN_USE.swap(true, Ordering::AcqRel),
        "Only one experimental KDE mirror is supported"
    );
    let reservation = Reservation;
    let session = Session::start(uuid)?;
    Toast::new(ToastTopic::System, None, Translation::from_raw_text("KDE mirror INACTIVE (orange). Click to activate; right trackpad/AltClick releases. Game input is NOT isolated.".into())).submit(app);
    let backend = KdeMirror {
        renderer: None,
        name: name.clone(),
        startup_failed: false,
        session,
        _reservation: reservation,
        owner: None,
        activation_click: false,
        activation_until: None,
        generation: None,
        held_button: None,
    };
    Ok(OverlayWindowConfig {
        name,
        category: OverlayCategory::Mirror,
        show_on_spawn: true,
        // Deliberately None: generic grabbing/clicking must not clear our input lease.
        input_focus: None,
        default_state: OverlayWindowState {
            interactable: true,
            grabbable: true,
            positioning: app.session.config.default_positioning.into(),
            transform: Affine3A::from_scale_rotation_translation(
                Vec3::ONE * app.session.config.default_overlay_scale,
                Quat::IDENTITY,
                vec3(0., 0.2, -0.35),
            ),
            ..Default::default()
        },
        ..OverlayWindowConfig::from_backend(Box::new(backend))
    })
}

struct KdeMirror {
    renderer: Option<ScreenBackend>,
    name: Arc<str>,
    startup_failed: bool,
    session: Session,
    _reservation: Reservation,
    owner: Option<usize>,
    activation_click: bool,
    activation_until: Option<Instant>,
    generation: Option<u64>,
    held_button: Option<u16>,
}
impl KdeMirror {
    fn revoke(&mut self, app: &mut AppState) {
        self.session.shared.revoke();
        self.owner = None;
        self.activation_click = false;
        self.activation_until = None;
        self.held_button = None;
        // The lease stays revoked until another deliberate focus selection.
        app.hid_provider.inner.commit();
    }
    fn geometry(&self) -> Option<Snapshot> {
        let snapshot = self.session.shared.snapshot()?;
        if !snapshot.visible || snapshot.transients {
            return None;
        }
        let extent = self.renderer.as_ref()?.capture_extent()?;
        for (actual, logical) in extent.into_iter().zip(&snapshot.geometry[2..]) {
            if (f64::from(actual) - logical * snapshot.scale).abs() > 1. {
                return None;
            }
        }
        Some(snapshot)
    }
    fn target_point(&self, hit: &PointerHit) -> Option<DVec2> {
        let snapshot = self.geometry()?;
        let point = map_point(&snapshot, [f64::from(hit.uv.x), f64::from(hit.uv.y)])?;
        self.session.shared.probe(point);
        if snapshot.probe_seq == 0 || snapshot.hit_uuid != self.session.shared.uuid {
            return None;
        }
        let approved = snapshot.probe?;
        // Inject the compositor-checked point, never the newer unchecked ray position.
        Some(DVec2::from_array(approved))
    }
    fn active(&self) -> bool {
        self.owner.is_some()
            && !self.activation_click
            && self.session.shared.valid()
            && self.geometry().is_some()
    }
}
impl OverlayBackend for KdeMirror {
    fn init(&mut self, _: &mut AppState) -> anyhow::Result<()> {
        Ok(())
    }
    fn pause(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.revoke(app);
        self.renderer.as_mut().map_or(Ok(()), |r| r.pause(app))
    }
    fn resume(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.revoke(app);
        self.renderer.as_mut().map_or(Ok(()), |r| r.resume(app))
    }
    fn should_render(&mut self, app: &mut AppState) -> anyhow::Result<ShouldRender> {
        if self.renderer.is_none() {
            if self.startup_failed {
                return Ok(ShouldRender::Unable);
            }
            match self.session.node.try_recv() {
                Ok(Ok(node)) => {
                    let capture = new_wlx_capture!(
                        app.gfx_extras.queue_capture,
                        PipewireCapture::new(self.name.clone(), node)
                    );
                    let mut renderer = ScreenBackend::new_raw(
                        self.name.clone(),
                        app.feats.xr_backend,
                        CaptureType::PipeWire,
                        capture,
                    );
                    renderer.set_experimental_status(app, false)?;
                    self.renderer = Some(renderer);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(ShouldRender::Unable),
                result => {
                    self.startup_failed = true;
                    log::error!("KDE mirror initialization refused: {result:?}");
                    Toast::new(ToastTopic::System, None, Translation::from_raw_text("KDE mirror refused capture. Check the development log; input remains disabled.")).submit(app);
                    return Ok(ShouldRender::Unable);
                }
            }
        }
        let render = self.renderer.as_mut().unwrap().should_render(app)?;
        let geometry = self.geometry();
        let generation = geometry.as_ref().map(|s| s.generation);
        self.session
            .shared
            .capture_generation
            .store(generation.unwrap_or(u64::MAX), Ordering::Release);
        if self.activation_until.is_some()
            && !self.activation_click
            && self.session.shared.target_valid()
        {
            self.activation_until = None;
            self.session.shared.accepted.store(true, Ordering::Release);
        }
        let release = self.owner.is_some_and(|owner| {
            let p = &app.input_state.pointers[owner];
            !p.tracked || p.now.alt_click || p.now.grab
        });
        if release
            || generation != self.generation
            || self
                .activation_until
                .is_some_and(|deadline| Instant::now() > deadline)
            || (self.owner.is_some()
                && !self.session.shared.valid()
                && self.activation_until.is_none())
        {
            self.revoke(app);
        }
        self.generation = generation;
        let active = self.active();
        let renderer = self.renderer.as_mut().unwrap();
        if renderer.set_experimental_status(app, active)? && renderer.frame_meta().is_some() {
            return Ok(ShouldRender::Should);
        }
        Ok(render)
    }
    fn render(&mut self, app: &mut AppState, rdr: &mut RenderResources) -> anyhow::Result<()> {
        self.renderer
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("KDE capture not ready"))?
            .render(app, rdr)
    }
    fn frame_meta(&mut self) -> Option<FrameMeta> {
        self.renderer.as_mut().and_then(|r| r.frame_meta())
    }
    fn notify(&mut self, app: &mut AppState, event: OverlayEventData) -> anyhow::Result<()> {
        self.renderer
            .as_mut()
            .map_or(Ok(()), |r| r.notify(app, event))
    }
    fn on_hover(&mut self, app: &mut AppState, hit: &PointerHit) -> HoverResult {
        if self
            .owner
            .is_some_and(|owner| !app.input_state.pointers[owner].tracked)
        {
            self.revoke(app);
        }
        // Probe without moving the actual pointer until identity and topmost hit agree.
        let point = self.target_point(hit);
        if point.is_none() && self.held_button.is_some() {
            self.revoke(app);
        }
        if self.active()
            && self.owner == Some(hit.pointer)
            && let Some(point) = point
        {
            app.hid_provider.inner.mouse_move(point);
        }
        HoverResult::consume()
    }
    fn on_left(&mut self, _: &mut AppState, _: usize) {} // Moving to the keyboard preserves the lease.
    fn on_pointer(&mut self, app: &mut AppState, hit: &PointerHit, down: bool) {
        if !down && self.activation_click && self.owner == Some(hit.pointer) {
            self.activation_click = false;
            return;
        }
        if down && self.owner.is_none() {
            if self.geometry().is_none() {
                return;
            }
            app.hid_provider
                .set_input_focus(app.wvr_server.as_mut(), InputFocus::PhysicalScreen);
            self.session.shared.accepted.store(false, Ordering::Release);
            self.session.shared.armed.store(true, Ordering::Release);
            self.session.shared.activate();
            app.hid_provider
                .lease
                .begin(self.session.shared.clone(), hit.pointer);
            self.owner = Some(hit.pointer);
            self.activation_click = true;
            self.activation_until = Some(Instant::now() + Duration::from_secs(1));
            return; // First click is activation only, never forwarded.
        }
        if self.owner != Some(hit.pointer) {
            return;
        }
        let mut button = match hit.mode {
            PointerMode::Right => MOUSE_RIGHT,
            PointerMode::Middle => MOUSE_MIDDLE,
            _ => MOUSE_LEFT,
        };
        if app.session.config.left_handed_mouse {
            button = match button {
                MOUSE_LEFT => MOUSE_RIGHT,
                MOUSE_RIGHT => MOUSE_LEFT,
                other => other,
            };
        }
        if !down {
            if let Some(button) = self.held_button.take() {
                app.hid_provider.inner.send_button(button, false);
            }
            return;
        }
        if self.active()
            && let Some(point) = self.target_point(hit)
        {
            let Some(snapshot) = self.geometry() else {
                return;
            };
            let Some(desired) = map_point(&snapshot, [f64::from(hit.uv.x), f64::from(hit.uv.y)])
            else {
                return;
            };
            if (DVec2::from_array(desired) - point).length() > 4. {
                return;
            }
            app.hid_provider.inner.mouse_move(point);
            app.hid_provider.inner.send_button(button, true);
            self.held_button = Some(button);
        }
    }
    fn on_scroll(&mut self, app: &mut AppState, hit: &PointerHit, mut delta: WheelDelta) {
        if self.active()
            && self.owner == Some(hit.pointer)
            && let Some(point) = self.target_point(hit)
        {
            app.hid_provider.inner.mouse_move(point);
            app.hid_provider.inner.commit();
            delta.x *= 60.;
            delta.y *= 60.;
            app.hid_provider.inner.wheel(delta);
        }
    }
    fn get_interaction_transform(&mut self) -> Option<Affine2> {
        self.renderer
            .as_mut()
            .and_then(|r| r.get_interaction_transform())
    }
    fn get_attrib(&self, attr: BackendAttrib) -> Option<BackendAttribValue> {
        self.renderer.as_ref().and_then(|r| r.get_attrib(attr))
    }
    fn set_attrib(&mut self, _: &mut AppState, _: BackendAttribValue) -> bool {
        false
    } // No stereo/crop transforms in v0.
}
impl Drop for KdeMirror {
    fn drop(&mut self) {
        self.session.shared.revoke();
    }
}
