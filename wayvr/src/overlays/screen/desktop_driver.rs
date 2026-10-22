//! Simulation-only desktop driver. RecordingProvider is the only output backend.
//! This module never connects to KWin, creates uinput devices, or initializes VR.
use super::{
    kde_interaction::MirrorInteraction,
    kde_session::{FRESHNESS, Shared, Snapshot, valid_geometry},
};
use crate::{
    backend::input::{PointerHit, PointerMode},
    subsystem::{
        hid::{VirtualKey, WheelDelta, XkbKeymap, provider::HidProvider},
        input::{HidWrapper, InputFocus},
    },
};
use glam::{DVec2, Vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, BufRead},
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Deserialize)]
struct Fixture {
    application_id: String,
    title: String,
    capture_uuid: String,
    extent: [u32; 2],
    snapshot: Snapshot,
}
#[derive(Default)]
struct Recording {
    events: Vec<Value>,
    buttons: BTreeSet<u16>,
    keys: BTreeSet<String>,
    modifiers: u8,
}
struct RecordingProvider(Arc<Mutex<Recording>>);
impl HidProvider for RecordingProvider {
    fn mouse_move(&mut self, p: DVec2) {
        self.0
            .lock()
            .unwrap()
            .events
            .push(json!({"mouse": p.to_array()}));
    }
    fn mouse_move_rel(&mut self, p: DVec2) {
        self.0
            .lock()
            .unwrap()
            .events
            .push(json!({"relative_mouse": p.to_array()}));
    }
    fn send_button(&mut self, button: u16, down: bool) {
        let mut r = self.0.lock().unwrap();
        if down {
            r.buttons.insert(button);
        } else {
            r.buttons.remove(&button);
        }
        r.events.push(json!({"button": button, "down": down}));
    }
    fn wheel(&mut self, delta: WheelDelta) {
        self.0
            .lock()
            .unwrap()
            .events
            .push(json!({"wheel": [delta.x, delta.y]}));
    }
    fn send_key(&mut self, key: VirtualKey, down: bool) {
        let mut r = self.0.lock().unwrap();
        let key = format!("{key:?}");
        if down {
            r.keys.insert(key.clone());
        } else {
            r.keys.remove(&key);
        }
        r.events.push(json!({"key": key, "down": down}));
    }
    fn set_modifiers(&mut self, mask: u8) {
        let mut r = self.0.lock().unwrap();
        r.modifiers = mask;
        r.events.push(json!({"modifiers": mask}));
    }
    fn set_desktop_extent(&mut self, _: DVec2) {}
    fn set_desktop_origin(&mut self, _: DVec2) {}
    fn set_keymap(&mut self, _: &XkbKeymap) {}
    fn commit(&mut self) {}
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Select {
        uuid: String,
    },
    Verify {
        uuid: String,
        application_id: String,
        disposable: bool,
    },
    Hover {
        uv: [f32; 2],
    },
    Button {
        uv: [f32; 2],
        down: bool,
        #[serde(default)]
        right: bool,
    },
    Key {
        key: VirtualKey,
        down: bool,
    },
    Modifiers {
        mask: u8,
    },
    Scroll {
        uv: [f32; 2],
        delta: [f32; 2],
    },
    Deactivate,
    Fault {
        kind: Fault,
    },
    Recover,
    Status,
    Quit,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Fault {
    FocusLoss,
    Movement,
    Resize,
    Scaling,
    CaptureLoss,
    TargetClosure,
    TrackingLoss,
    StaleMetadata,
    Occlusion,
    Popup,
}

struct Driver {
    fixture: Fixture,
    shared: Arc<Shared>,
    interaction: MirrorInteraction,
    hid: HidWrapper,
    recording: Arc<Mutex<Recording>>,
    selected: bool,
    verified: bool,
    capture_alive: bool,
    tracked: bool,
    stale: bool,
    occluded: bool,
    last_uv: [f32; 2],
}
impl Driver {
    fn new() -> anyhow::Result<Self> {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../res/kde_driver_fixture.json"))?;
        let shared = Shared::disconnected(fixture.snapshot.uuid.clone());
        let interaction = MirrorInteraction::new(shared.clone());
        let recording = Arc::new(Mutex::new(Recording::default()));
        let mut hid =
            HidWrapper::from_recording_provider(Box::new(RecordingProvider(recording.clone())));
        // Activation must switch this through the same HidWrapper route used by VR.
        hid.set_input_focus(None, InputFocus::WayVR);
        Ok(Self {
            fixture,
            shared,
            interaction,
            hid,
            recording,
            selected: false,
            verified: false,
            capture_alive: true,
            tracked: true,
            stale: false,
            occluded: false,
            last_uv: [0.5, 0.5],
        })
    }
    fn sync(&mut self) {
        let (activate, probe, sequence) = self.shared.simulation_commands();
        if activate && self.fixture.snapshot.visible {
            self.fixture.snapshot.active = true;
        }
        self.fixture.snapshot.probe = probe;
        self.fixture.snapshot.probe_seq = sequence;
        self.fixture.snapshot.hit_uuid = if self.occluded {
            "{22222222-2222-4222-8222-222222222222}".into()
        } else {
            self.fixture.snapshot.uuid.clone()
        };
        self.shared
            .alive
            .store(self.capture_alive, Ordering::Release);
        self.shared.simulate_snapshot(
            self.fixture.snapshot.clone(),
            if self.stale {
                FRESHNESS + Duration::from_millis(1)
            } else {
                Duration::ZERO
            },
        );
        self.interaction.tick(
            self.capture_alive.then_some(self.fixture.extent),
            [self.tracked, false],
            [false; 2],
            Instant::now(),
            &mut self.hid,
        );
    }
    fn hit(&self, uv: [f32; 2], right: bool) -> PointerHit {
        PointerHit {
            uv: Vec2::from_array(uv),
            mode: if right {
                PointerMode::Right
            } else {
                PointerMode::Left
            },
            ..Default::default()
        }
    }
    fn response(&self, accepted: bool, reason: &str) -> Value {
        let mut r = self.recording.lock().unwrap();
        json!({"mode": "simulation_only", "accepted": accepted, "reason": reason,
            "selected": self.selected, "verified": self.verified,
            "active": self.interaction.active(), "border_logic": if self.interaction.active() { "green" } else { "orange" },
            "keyboard_route": format!("{:?}", self.hid.get_input_focus()),
            "events": std::mem::take(&mut r.events),
            "held_buttons": r.buttons, "held_keys": r.keys, "modifiers": r.modifiers })
    }
    fn step(&mut self, command: Command) -> Value {
        self.sync();
        let input = matches!(
            command,
            Command::Hover { .. }
                | Command::Button { .. }
                | Command::Key { .. }
                | Command::Modifiers { .. }
                | Command::Scroll { .. }
        );
        if input && (!self.selected || !self.verified) {
            return self.response(
                false,
                "Explicit selection and KWrite/capture verification are required",
            );
        }
        let keyboard = matches!(command, Command::Key { .. } | Command::Modifiers { .. });
        if keyboard && !self.interaction.active() {
            return self.response(false, "Activate and release the first click before typing");
        }
        match command {
            Command::Select { uuid } => {
                self.interaction.deactivate(&mut self.hid);
                self.interaction = MirrorInteraction::new(self.shared.clone());
                self.selected = uuid == self.fixture.snapshot.uuid;
                self.verified = false;
                if !self.selected {
                    return self.response(false, "UUID is not the disposable KWrite fixture");
                }
            }
            Command::Verify {
                uuid,
                application_id,
                disposable,
            } => {
                let valid = self.selected
                    && disposable
                    && uuid == self.fixture.snapshot.uuid
                    && uuid == self.fixture.capture_uuid
                    && application_id == self.fixture.application_id
                    && application_id == "org.kde.kwrite"
                    && self.fixture.snapshot.visible
                    && valid_geometry(&self.fixture.snapshot);
                if !valid {
                    self.interaction.deactivate(&mut self.hid);
                    self.verified = false;
                    return self.response(
                        false,
                        "KWrite identity, capture UUID and disposable confirmation must all agree",
                    );
                }
                self.verified = self.interaction.verify_target(&uuid).is_ok();
            }
            Command::Hover { uv } => {
                self.last_uv = uv;
                let hit = self.hit(uv, false);
                self.interaction.hover(&mut self.hid, &hit);
            }
            Command::Button { uv, down, right } => {
                self.last_uv = uv;
                let hit = self.hit(uv, right);
                self.interaction
                    .pointer(&mut self.hid, None, &hit, down, false, Instant::now());
            }
            Command::Key { key, down } => {
                if !matches!(
                    key,
                    VirtualKey::A
                        | VirtualKey::B
                        | VirtualKey::Space
                        | VirtualKey::Return
                        | VirtualKey::BackSpace
                ) {
                    return self.response(
                        false,
                        "Driver keys are limited to A, B, Space, Return and BackSpace",
                    );
                }
                self.hid.send_key_routed(None, key, down);
            }
            Command::Modifiers { mask } => {
                if mask & !0xdf != 0 {
                    return self.response(false, "Unknown modifier bits");
                }
                self.hid.set_modifiers_routed(None, mask);
            }
            Command::Scroll { uv, delta } => {
                if !delta.iter().all(|v| v.is_finite() && v.abs() <= 1.) {
                    return self.response(
                        false,
                        "Scroll is limited to a finite controller-sized delta",
                    );
                }
                let hit = self.hit(uv, false);
                self.interaction.scroll(
                    &mut self.hid,
                    &hit,
                    WheelDelta {
                        x: delta[0],
                        y: delta[1],
                    },
                );
            }
            Command::Deactivate | Command::Quit => self.interaction.deactivate(&mut self.hid),
            Command::Fault { kind } => match kind {
                Fault::FocusLoss => self.fixture.snapshot.active = false,
                Fault::Movement => {
                    self.fixture.snapshot.geometry[0] += 100.;
                    self.fixture.snapshot.generation += 1;
                }
                Fault::Resize => {
                    self.fixture.snapshot.geometry[2] += 100.;
                    self.fixture.extent[0] += 150;
                    self.fixture.snapshot.generation += 1;
                }
                Fault::Scaling => {
                    self.fixture.snapshot.scale = 2.;
                    self.fixture.extent = [1600, 1200];
                    self.fixture.snapshot.generation += 1;
                }
                Fault::CaptureLoss => self.capture_alive = false,
                Fault::TargetClosure => {
                    self.fixture.snapshot.visible = false;
                    self.verified = false;
                }
                Fault::TrackingLoss => self.tracked = false,
                Fault::StaleMetadata => self.stale = true,
                Fault::Occlusion => self.occluded = true,
                Fault::Popup => self.fixture.snapshot.transients = true,
            },
            Command::Recover => {
                let original: Fixture =
                    serde_json::from_str(include_str!("../../res/kde_driver_fixture.json"))
                        .unwrap();
                self.fixture.snapshot.geometry = original.snapshot.geometry;
                self.fixture.snapshot.scale = original.snapshot.scale;
                self.fixture.snapshot.active = false;
                self.fixture.snapshot.visible = true;
                self.fixture.snapshot.transients = false;
                self.fixture.snapshot.generation += 1;
                self.fixture.extent = original.extent;
                self.capture_alive = true;
                self.tracked = true;
                self.stale = false;
                self.occluded = false;
            }
            Command::Status => {}
        }
        self.sync();
        if self.occluded {
            let hit = self.hit(self.last_uv, false);
            self.interaction.hover(&mut self.hid, &hit);
        }
        self.hid.inner.commit();
        self.response(
            true,
            "Simulated only; no KDE or desktop input backend exists in this driver",
        )
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        self.interaction.deactivate(&mut self.hid);
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut driver = Driver::new()?;
    println!(
        "{}",
        json!({"mode": "simulation_only", "real_input": false, "kde_connection": false,
        "vr_runtime": false, "fixture": {"uuid": driver.fixture.snapshot.uuid, "application_id": driver.fixture.application_id, "title": driver.fixture.title},
        "instructions": "Select and verify the fixture UUID before input; commands are JSON lines. There is no live mode."})
    );
    let outcome = (|| -> Result<(), Box<dyn std::error::Error>> {
        for line in io::stdin().lock().lines() {
            let line = line?;
            if line.len() > 8192 {
                return Err("Driver command exceeds 8192 bytes".into());
            }
            let command: Command = serde_json::from_str(&line)?;
            let quit = matches!(command, Command::Quit);
            println!("{}", driver.step(command));
            if quit {
                break;
            }
        }
        Ok(())
    })();
    driver.interaction.deactivate(&mut driver.hid);
    println!(
        "{}",
        driver.response(outcome.is_ok(), "Simulation ended; recorded input released")
    );
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    fn select_and_verify(driver: &mut Driver) {
        let uuid = driver.fixture.snapshot.uuid.clone();
        assert_eq!(
            driver.step(Command::Select { uuid: uuid.clone() })["accepted"],
            true
        );
        assert_eq!(
            driver.step(Command::Verify {
                uuid,
                application_id: "org.kde.kwrite".into(),
                disposable: true
            })["verified"],
            true
        );
    }
    fn activate(driver: &mut Driver) {
        let down = driver.step(Command::Button {
            uv: [0.5, 0.5],
            down: true,
            right: false,
        });
        assert_eq!(down["active"], false);
        assert_eq!(
            down["events"],
            json!([]),
            "activation press must not leak a button or keyboard event"
        );
        let up = driver.step(Command::Button {
            uv: [0.5, 0.5],
            down: false,
            right: false,
        });
        assert_eq!(up["active"], true);
        assert_eq!(up["events"], json!([]));
        assert_eq!(up["keyboard_route"], "PhysicalScreen");
        driver.step(Command::Hover { uv: [0.5, 0.5] });
    }
    fn active_driver() -> Driver {
        let mut d = Driver::new().unwrap();
        select_and_verify(&mut d);
        activate(&mut d);
        d
    }
    fn hold_inputs(d: &mut Driver) {
        let down = d.step(Command::Button {
            uv: [0.5, 0.5],
            down: true,
            right: false,
        });
        assert_eq!(down["held_buttons"], json!([272]));
        d.step(Command::Key {
            key: VirtualKey::A,
            down: true,
        });
        let mods = d.step(Command::Modifiers { mask: 1 });
        assert_eq!(mods["held_keys"], json!(["A"]));
        assert_eq!(mods["modifiers"], 1);
    }
    fn released(response: &Value) {
        assert_eq!(response["active"], false);
        assert_eq!(response["held_buttons"], json!([]));
        assert_eq!(response["held_keys"], json!([]));
        assert_eq!(response["modifiers"], 0);
    }
    #[test]
    fn selection_application_disposable_and_capture_identity_are_mandatory() {
        let mut d = Driver::new().unwrap();
        for command in [
            Command::Key {
                key: VirtualKey::A,
                down: true,
            },
            Command::Button {
                uv: [0.5, 0.5],
                down: true,
                right: false,
            },
        ] {
            let response = d.step(command);
            assert_eq!(response["accepted"], false);
            assert_eq!(response["events"], json!([]));
        }
        let uuid = d.fixture.snapshot.uuid.clone();
        d.step(Command::Select { uuid: uuid.clone() });
        let wrong_app = d.step(Command::Verify {
            uuid: uuid.clone(),
            application_id: "com.openai.ChatGPT".into(),
            disposable: true,
        });
        assert_eq!(wrong_app["verified"], false);
        assert_eq!(
            d.step(Command::Verify {
                uuid: uuid.clone(),
                application_id: "org.kde.kwrite".into(),
                disposable: false
            })["verified"],
            false
        );
        d.fixture.capture_uuid = "{22222222-2222-4222-8222-222222222222}".into();
        assert_eq!(
            d.step(Command::Verify {
                uuid,
                application_id: "org.kde.kwrite".into(),
                disposable: true
            })["verified"],
            false
        );
    }
    #[test]
    fn hover_never_activates_and_first_click_only_changes_the_keyboard_route() {
        let mut d = Driver::new().unwrap();
        select_and_verify(&mut d);
        let hover = d.step(Command::Hover { uv: [0.5, 0.5] });
        assert_eq!(hover["active"], false);
        assert_eq!(hover["events"], json!([]));
        assert_eq!(hover["keyboard_route"], "WayVR");
        assert_eq!(
            d.step(Command::Key {
                key: VirtualKey::A,
                down: true
            })["accepted"],
            false
        );
        activate(&mut d);
    }
    #[test]
    fn mapped_pointer_click_scroll_and_keyboard_reach_only_the_recording_provider() {
        let mut d = active_driver();
        let hover = d.step(Command::Hover { uv: [0.5, 0.5] });
        assert_eq!(hover["events"], json!([{"mouse": [-400., 400.]}]));
        let down = d.step(Command::Button {
            uv: [0.5, 0.5],
            down: true,
            right: true,
        });
        assert_eq!(down["held_buttons"], json!([273]));
        // Release must use the original pressed button even when the modifier changes.
        let up = d.step(Command::Button {
            uv: [0.5, 0.5],
            down: false,
            right: false,
        });
        assert_eq!(up["held_buttons"], json!([]));
        assert!(
            up["events"]
                .as_array()
                .unwrap()
                .contains(&json!({"button":273,"down":false}))
        );
        let wheel = d.step(Command::Scroll {
            uv: [0.5, 0.5],
            delta: [0., 1.],
        });
        assert!(
            wheel["events"]
                .as_array()
                .unwrap()
                .contains(&json!({"wheel":[0.,60.]}))
        );
        let key = d.step(Command::Key {
            key: VirtualKey::B,
            down: true,
        });
        assert_eq!(key["events"], json!([{"key":"B","down":true}]));
        released(&d.step(Command::Deactivate));
    }
    #[test]
    fn recoverable_failure_conditions_release_every_recorded_input_and_do_not_auto_resume() {
        for kind in [
            Fault::FocusLoss,
            Fault::Movement,
            Fault::Resize,
            Fault::Scaling,
            Fault::CaptureLoss,
            Fault::TargetClosure,
            Fault::TrackingLoss,
            Fault::StaleMetadata,
            Fault::Occlusion,
            Fault::Popup,
        ] {
            let mut d = active_driver();
            hold_inputs(&mut d);
            let result = d.step(Command::Fault { kind });
            released(&result);
            released(&d.step(Command::Recover));
            assert_eq!(
                d.step(Command::Key {
                    key: VirtualKey::A,
                    down: true
                })["accepted"],
                false
            );
        }
    }
    #[test]
    fn explicit_deactivation_and_driver_drop_release_recorded_input() {
        let mut d = active_driver();
        hold_inputs(&mut d);
        released(&d.step(Command::Deactivate));
        activate(&mut d);
        hold_inputs(&mut d);
        let recorder = d.recording.clone();
        drop(d);
        let r = recorder.lock().unwrap();
        assert!(r.buttons.is_empty() && r.keys.is_empty() && r.modifiers == 0);
    }
}
