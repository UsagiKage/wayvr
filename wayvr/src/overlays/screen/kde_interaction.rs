//! Shared by the VR mirror and the development-only desktop simulator.
//! Contains no capture setup, device creation, runtime initialization or services.
use super::kde_session::{Shared, Snapshot, map_point, valid_geometry};
use crate::{
    backend::{
        input::{PointerHit, PointerMode},
        wayvr::WvrServerState,
    },
    subsystem::{
        hid::{MOUSE_LEFT, MOUSE_MIDDLE, MOUSE_RIGHT, WheelDelta},
        input::{HidWrapper, InputFocus},
        input_lease::InputPermit,
    },
};
use glam::DVec2;
use std::{
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

pub struct MirrorInteraction {
    shared: Arc<Shared>,
    verified: bool,
    extent: Option<[u32; 2]>,
    owner: Option<usize>,
    activation_click: bool,
    activation_until: Option<Instant>,
    generation: Option<u64>,
    held_button: Option<u16>,
}
impl MirrorInteraction {
    pub fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            verified: false,
            extent: None,
            owner: None,
            activation_click: false,
            activation_until: None,
            generation: None,
            held_button: None,
        }
    }
    /// Caller must obtain this UUID from the actual capture association, never a title match.
    pub fn verify_target(&mut self, capture_uuid: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            uuid::Uuid::parse_str(capture_uuid).is_ok() && capture_uuid == self.shared.uuid,
            "Capture identity does not match the explicitly selected target"
        );
        self.verified = true;
        Ok(())
    }
    pub fn deactivate(&mut self, hid: &mut HidWrapper) {
        self.shared.revoke();
        self.owner = None;
        self.activation_click = false;
        self.activation_until = None;
        self.held_button = None;
        hid.inner.commit();
    }
    pub fn geometry(&self) -> Option<Snapshot> {
        if !self.verified {
            return None;
        }
        let s = self.shared.snapshot()?;
        if s.uuid != self.shared.uuid || !s.visible || s.transients || !valid_geometry(&s) {
            return None;
        }
        for (actual, logical) in self.extent?.into_iter().zip(&s.geometry[2..]) {
            if (f64::from(actual) - logical * s.scale).abs() > 1. {
                return None;
            }
        }
        Some(s)
    }
    pub fn active(&self) -> bool {
        self.owner.is_some()
            && !self.activation_click
            && self.shared.valid()
            && self.geometry().is_some()
    }
    pub fn tick(
        &mut self,
        extent: Option<[u32; 2]>,
        tracked: [bool; 2],
        release: [bool; 2],
        now: Instant,
        hid: &mut HidWrapper,
    ) {
        self.extent = extent;
        hid.lease.tracking(tracked);
        let generation = self.geometry().map(|s| s.generation);
        self.shared
            .capture_generation
            .store(generation.unwrap_or(u64::MAX), Ordering::Release);
        if self.activation_until.is_some() && !self.activation_click && self.shared.target_valid() {
            self.activation_until = None;
            self.shared.accepted.store(true, Ordering::Release);
        }
        let release = self
            .owner
            .is_some_and(|owner| !tracked[owner] || release[owner]);
        if release
            || generation != self.generation
            || self.activation_until.is_some_and(|deadline| now > deadline)
            || (self.owner.is_some() && !self.shared.valid() && self.activation_until.is_none())
        {
            self.deactivate(hid);
        }
        self.generation = generation;
        // Keep expiry/capture rejection effective even without a new pointer event.
        hid.inner.commit();
    }
    fn target_point(&self, hit: &PointerHit) -> Option<DVec2> {
        let s = self.geometry()?;
        let point = map_point(&s, [f64::from(hit.uv.x), f64::from(hit.uv.y)])?;
        self.shared.probe(point);
        if s.probe_seq == 0 || s.hit_uuid != self.shared.uuid {
            return None;
        }
        let approved = s.probe?;
        if !approved.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(DVec2::from_array(approved))
    }
    pub fn hover(&mut self, hid: &mut HidWrapper, hit: &PointerHit) {
        let point = self.target_point(hit);
        if point.is_none() && self.held_button.is_some() {
            self.deactivate(hid);
        }
        if self.active()
            && self.owner == Some(hit.pointer)
            && let Some(point) = point
        {
            hid.inner.mouse_move(point);
        }
    }
    pub fn pointer(
        &mut self,
        hid: &mut HidWrapper,
        server: Option<&mut WvrServerState>,
        hit: &PointerHit,
        down: bool,
        left_handed: bool,
        now: Instant,
    ) {
        if hit.pointer >= 2 {
            return;
        }
        if !down && self.activation_click && self.owner == Some(hit.pointer) {
            self.activation_click = false;
            return;
        }
        if down && self.owner.is_none() {
            if self.geometry().is_none() {
                return;
            }
            hid.set_input_focus(server, InputFocus::PhysicalScreen);
            self.shared.accepted.store(false, Ordering::Release);
            self.shared.armed.store(true, Ordering::Release);
            self.shared.activate();
            hid.lease.begin(self.shared.clone(), hit.pointer);
            self.owner = Some(hit.pointer);
            self.activation_click = true;
            self.activation_until = Some(now + Duration::from_secs(1));
            return;
        }
        if self.owner != Some(hit.pointer) {
            return;
        }
        let mut button = match hit.mode {
            PointerMode::Right => MOUSE_RIGHT,
            PointerMode::Middle => MOUSE_MIDDLE,
            _ => MOUSE_LEFT,
        };
        if left_handed {
            button = match button {
                MOUSE_LEFT => MOUSE_RIGHT,
                MOUSE_RIGHT => MOUSE_LEFT,
                other => other,
            };
        }
        if !down {
            if let Some(button) = self.held_button.take() {
                hid.inner.send_button(button, false);
            }
            return;
        }
        if self.active()
            && let Some(point) = self.target_point(hit)
        {
            let Some(s) = self.geometry() else {
                return;
            };
            let Some(desired) = map_point(&s, [f64::from(hit.uv.x), f64::from(hit.uv.y)]) else {
                return;
            };
            if (DVec2::from_array(desired) - point).length() > 4. {
                return;
            }
            hid.inner.mouse_move(point);
            hid.inner.send_button(button, true);
            self.held_button = Some(button);
        }
    }
    pub fn scroll(&mut self, hid: &mut HidWrapper, hit: &PointerHit, mut delta: WheelDelta) {
        if self.active()
            && self.owner == Some(hit.pointer)
            && let Some(point) = self.target_point(hit)
        {
            hid.inner.mouse_move(point);
            hid.inner.commit();
            delta.x *= 60.;
            delta.y *= 60.;
            hid.inner.wheel(delta);
        }
    }
}
impl Drop for MirrorInteraction {
    fn drop(&mut self) {
        self.shared.revoke();
    }
}
