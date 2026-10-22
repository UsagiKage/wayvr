//! One explicitly selected existing KWin window. Experimental and fail-closed.
use super::{
    backend::{CaptureType, ScreenBackend},
    capture::{MainThreadWlxCapture, new_wlx_capture},
    kde_interaction::MirrorInteraction,
    kde_session::Session,
};
use crate::{
    backend::input::{HoverResult, PointerHit},
    overlays::toast::Toast,
    state::AppState,
    subsystem::{hid::WheelDelta, input_lease::InputPermit},
    windowing::{
        backend::{FrameMeta, OverlayBackend, OverlayEventData, RenderResources, ShouldRender},
        window::{OverlayCategory, OverlayWindowConfig},
    },
};
use glam::{Affine2, Affine3A, Quat, Vec3, vec3};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
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
        interaction: MirrorInteraction::new(session.shared.clone()),
        session,
        _reservation: reservation,
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
    interaction: MirrorInteraction,
}
impl OverlayBackend for KdeMirror {
    fn init(&mut self, _: &mut AppState) -> anyhow::Result<()> {
        Ok(())
    }
    fn pause(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.interaction.deactivate(&mut app.hid_provider);
        self.renderer.as_mut().map_or(Ok(()), |r| r.pause(app))
    }
    fn resume(&mut self, app: &mut AppState) -> anyhow::Result<()> {
        self.interaction.deactivate(&mut app.hid_provider);
        self.renderer.as_mut().map_or(Ok(()), |r| r.resume(app))
    }
    fn should_render(&mut self, app: &mut AppState) -> anyhow::Result<ShouldRender> {
        if self.renderer.is_none() {
            if self.startup_failed {
                return Ok(ShouldRender::Unable);
            }
            match self.session.node.try_recv() {
                Ok(Ok(node)) => {
                    self.interaction.verify_target(&self.session.shared.uuid)?;
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
        self.interaction.tick(
            self.renderer.as_ref().and_then(|r| r.capture_extent()),
            [
                app.input_state.pointers[0].tracked,
                app.input_state.pointers[1].tracked,
            ],
            [
                app.input_state.pointers[0].now.alt_click || app.input_state.pointers[0].now.grab,
                app.input_state.pointers[1].now.alt_click || app.input_state.pointers[1].now.grab,
            ],
            Instant::now(),
            &mut app.hid_provider,
        );
        let active = self.interaction.active();
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
        self.interaction.hover(&mut app.hid_provider, hit);
        HoverResult::consume()
    }
    fn on_left(&mut self, _: &mut AppState, _: usize) {} // Keyboard retains the desktop lease.
    fn on_pointer(&mut self, app: &mut AppState, hit: &PointerHit, down: bool) {
        self.interaction.pointer(
            &mut app.hid_provider,
            app.wvr_server.as_mut(),
            hit,
            down,
            app.session.config.left_handed_mouse,
            Instant::now(),
        );
    }
    fn on_scroll(&mut self, app: &mut AppState, hit: &PointerHit, delta: WheelDelta) {
        self.interaction.scroll(&mut app.hid_provider, hit, delta);
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
