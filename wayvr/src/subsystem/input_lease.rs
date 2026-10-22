//! Tracks only input injected by this process. An expired lease fails closed.
use std::sync::{Arc, Mutex};

use glam::DVec2;

use super::hid::{VirtualKey, WheelDelta, XkbKeymap, provider::HidProvider};

pub trait InputPermit: Send + Sync {
    fn valid(&self) -> bool;
    fn revoke(&self);
}

#[derive(Default)]
struct Lease {
    permit: Option<Arc<dyn InputPermit>>,
    owner: usize,
    tracked: bool,
    release_pending: bool,
    buttons: Vec<u16>,
    keys: Vec<VirtualKey>,
    modifiers: u8,
    ever_allowed: bool,
}

#[derive(Clone, Default)]
pub struct LeaseController(Arc<Mutex<Lease>>);

impl LeaseController {
    pub fn begin(&self, permit: Arc<dyn InputPermit>, owner: usize) {
        let mut lease = self.0.lock().unwrap();
        if let Some(old) = &lease.permit {
            old.revoke();
        }
        lease.release_pending = true;
        lease.permit = Some(permit);
        lease.owner = owner;
        lease.tracked = true;
        lease.ever_allowed = false;
    }

    pub fn clear(&self) {
        let mut lease = self.0.lock().unwrap();
        if let Some(old) = lease.permit.take() {
            old.revoke();
        }
        lease.release_pending = true;
    }

    pub fn tracking(&self, tracked: [bool; 2]) {
        let mut lease = self.0.lock().unwrap();
        lease.tracked = tracked.get(lease.owner).copied().unwrap_or(false);
        if !lease.tracked
            && let Some(permit) = &lease.permit
        {
            permit.revoke();
        }
    }
}

pub struct LeasedProvider {
    inner: Box<dyn HidProvider>,
    controller: LeaseController,
}

impl LeasedProvider {
    pub fn wrap(inner: Box<dyn HidProvider>) -> (Box<dyn HidProvider>, LeaseController) {
        let controller = LeaseController::default();
        (
            Box::new(Self {
                inner,
                controller: controller.clone(),
            }),
            controller,
        )
    }

    fn ready(&mut self) -> bool {
        let mut lease = self.controller.0.lock().unwrap();
        let allowed = lease
            .permit
            .as_ref()
            .is_none_or(|p| lease.tracked && p.valid());
        if !allowed
            && lease.ever_allowed
            && let Some(permit) = &lease.permit
        {
            permit.revoke();
        }
        if allowed && lease.permit.is_some() {
            lease.ever_allowed = true;
        }
        if lease.release_pending || !allowed {
            // Flush each release: the underlying uinput provider queues one button.
            self.inner.commit();
            for button in lease.buttons.drain(..) {
                self.inner.send_button(button, false);
                self.inner.commit();
            }
            for key in lease.keys.drain(..) {
                self.inner.send_key(key, false);
            }
            if lease.modifiers != 0 {
                self.inner.set_modifiers(0);
                lease.modifiers = 0;
            }
            lease.release_pending = false;
        }
        allowed
    }
}

impl Drop for LeasedProvider {
    fn drop(&mut self) {
        self.controller.clear();
        self.ready();
        self.inner.commit();
    }
}

impl HidProvider for LeasedProvider {
    fn mouse_move(&mut self, pos: DVec2) {
        if self.ready() {
            self.inner.mouse_move(pos);
        }
    }
    fn mouse_move_rel(&mut self, pos: DVec2) {
        if self.ready() {
            self.inner.mouse_move_rel(pos);
        }
    }
    fn wheel(&mut self, delta: WheelDelta) {
        if self.ready() {
            self.inner.wheel(delta);
        }
    }
    fn send_button(&mut self, button: u16, down: bool) {
        if !self.ready() {
            return;
        }
        let mut lease = self.controller.0.lock().unwrap();
        if lease.permit.is_some() {
            if down && !lease.buttons.contains(&button) {
                lease.buttons.push(button);
            }
            if !down {
                lease.buttons.retain(|b| *b != button);
            }
        }
        self.inner.send_button(button, down);
        self.inner.commit();
    }
    fn send_key(&mut self, key: VirtualKey, down: bool) {
        if !self.ready() {
            return;
        }
        let mut lease = self.controller.0.lock().unwrap();
        if lease.permit.is_some() {
            if down && !lease.keys.contains(&key) {
                lease.keys.push(key);
            }
            if !down {
                lease.keys.retain(|k| *k != key);
            }
        }
        self.inner.send_key(key, down);
    }
    fn set_modifiers(&mut self, mods: u8) {
        if !self.ready() {
            return;
        }
        let mut lease = self.controller.0.lock().unwrap();
        if lease.permit.is_some() {
            lease.modifiers = mods;
        }
        self.inner.set_modifiers(mods);
    }
    fn set_desktop_extent(&mut self, v: DVec2) {
        self.inner.set_desktop_extent(v);
    }
    fn set_desktop_origin(&mut self, v: DVec2) {
        self.inner.set_desktop_origin(v);
    }
    fn set_keymap(&mut self, v: &XkbKeymap) {
        self.inner.set_keymap(v);
    }
    fn commit(&mut self) {
        self.ready();
        self.inner.commit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct Permit(AtomicBool);
    impl InputPermit for Permit {
        fn valid(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
        fn revoke(&self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    struct Recorder(Arc<Mutex<Vec<String>>>);
    impl HidProvider for Recorder {
        fn mouse_move(&mut self, _: DVec2) {}
        fn mouse_move_rel(&mut self, _: DVec2) {}
        fn wheel(&mut self, _: WheelDelta) {}
        fn set_desktop_extent(&mut self, _: DVec2) {}
        fn set_desktop_origin(&mut self, _: DVec2) {}
        fn set_keymap(&mut self, _: &XkbKeymap) {}
        fn commit(&mut self) {}
        fn send_button(&mut self, b: u16, d: bool) {
            self.0.lock().unwrap().push(format!("button:{b}:{d}"));
        }
        fn send_key(&mut self, k: VirtualKey, d: bool) {
            self.0.lock().unwrap().push(format!("key:{k:?}:{d}"));
        }
        fn set_modifiers(&mut self, m: u8) {
            self.0.lock().unwrap().push(format!("mods:{m}"));
        }
    }
    #[test]
    fn revocation_releases_every_injected_button_key_and_modifier_and_blocks_new_input() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (mut provider, controller) = LeasedProvider::wrap(Box::new(Recorder(events.clone())));
        let permit = Arc::new(Permit(AtomicBool::new(true)));
        controller.begin(permit.clone(), 0);
        provider.send_button(272, true);
        provider.send_button(273, true);
        provider.send_key(VirtualKey::A, true);
        provider.set_modifiers(4);
        permit.revoke();
        provider.commit();
        let count = events.lock().unwrap().len();
        provider.send_key(VirtualKey::B, true);
        provider.send_button(274, true);
        let events = events.lock().unwrap();
        assert_eq!(events.len(), count);
        for expected in [
            "button:272:false",
            "button:273:false",
            "key:A:false",
            "mods:0",
        ] {
            assert!(
                events.iter().any(|e| e == expected),
                "missing {expected}: {events:?}"
            );
        }
    }
    #[test]
    fn tracking_loss_revokes_until_a_new_explicit_lease() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (mut provider, controller) = LeasedProvider::wrap(Box::new(Recorder(events.clone())));
        controller.begin(Arc::new(Permit(AtomicBool::new(true))), 1);
        provider.send_button(272, true);
        controller.tracking([true, false]);
        provider.commit();
        controller.tracking([true, true]);
        provider.send_button(272, true);
        assert_eq!(
            *events.lock().unwrap(),
            ["button:272:true", "button:272:false"]
        );
    }
}
