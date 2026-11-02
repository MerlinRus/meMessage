use crate::model::{Message, SendState};
use std::{
    collections::{HashMap, HashSet},
    time::Instant,
};

pub const RESPONSE: f32 = 0.4;
pub const POPUP_TIME: f32 = 0.16;
#[derive(Default)]
pub struct Spring {
    value: f32,
    velocity: f32,
    initialized: bool,
}
impl Spring {
    pub fn step(&mut self, target: f32, dt: f32, enabled: bool) -> (f32, bool) {
        if !self.initialized || !enabled {
            self.initialized = true;
            self.value = target;
            self.velocity = 0.;
            return (target, false);
        }
        let omega = std::f32::consts::TAU / RESPONSE;
        let dt = dt.clamp(0., 0.05);
        let displacement = self.value - target;
        let coefficient = self.velocity + omega * displacement;
        let decay = (-omega * dt).exp();
        self.value = target + (displacement + coefficient * dt) * decay;
        self.velocity = (self.velocity - omega * coefficient * dt) * decay;
        let active = (self.value - target).abs() > 0.05 || self.velocity.abs() > 0.1;
        if !active {
            self.value = target;
            self.velocity = 0.;
        }
        (self.value, active)
    }
}
pub struct Motion {
    pub enabled: bool,
    pub composer: Spring,
    arrivals: HashMap<String, Instant>,
    dialogue: Option<Instant>,
}
impl Motion {
    pub fn new() -> Self {
        let enabled = windows::UI::ViewManagement::UISettings::new()
            .and_then(|settings| settings.AnimationsEnabled())
            .unwrap_or(true);
        Self {
            enabled,
            composer: Spring::default(),
            arrivals: HashMap::new(),
            dialogue: None,
        }
    }
    pub fn register(&mut self, old: &[Message], new: &[Message]) {
        let known: HashSet<_> = old.iter().map(|m| m.id.as_str()).collect();
        for message in new {
            if !known.contains(message.id.as_str())
                && !old.iter().any(|previous| {
                    previous.outgoing
                        && previous.state == SendState::Pending
                        && message.outgoing
                        && previous.phone == message.phone
                        && previous.text == message.text
                        && previous.timestamp == message.timestamp
                })
            {
                self.arrivals.insert(message.id.clone(), Instant::now());
            }
        }
        self.arrivals
            .retain(|_, start| start.elapsed().as_secs_f32() < RESPONSE);
    }
    pub fn message(&self, id: &str) -> (f32, f32, bool) {
        let Some(start) = self.arrivals.get(id).filter(|_| self.enabled) else {
            return (1., 1., false);
        };
        let t = start.elapsed().as_secs_f32();
        if t >= RESPONSE {
            return (1., 1., false);
        }
        let x = std::f32::consts::TAU * t / RESPONSE;
        (1. - (1. + x) * (-x).exp(), (t / 0.12).clamp(0., 1.), true)
    }
    pub fn switch_dialogue(&mut self) {
        self.dialogue = Some(Instant::now());
    }
    pub fn dialogue_opacity(&self) -> (f32, bool) {
        let Some(start) = self.dialogue.filter(|_| self.enabled) else {
            return (1., false);
        };
        let t = (start.elapsed().as_secs_f32() / 0.12).clamp(0., 1.);
        (1. - (1. - t).powi(3), t < 1.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spring_is_continuous_on_retarget_and_settles_without_overshoot() {
        let mut spring = Spring::default();
        assert_eq!(spring.step(40., 0.016, true).0, 40.);
        let (mut previous, mut active) = (40., true);
        for _ in 0..8 {
            let (value, next) = spring.step(128., 0.016, true);
            assert!(value >= previous && value <= 128.);
            previous = value;
            active = next;
        }
        assert!(active);
        let (value, _) = spring.step(40., 0.016, true);
        assert!((value - previous).abs() < 10.);
        for _ in 0..100 {
            spring.step(40., 0.016, true);
        }
        assert_eq!(spring.step(40., 0.016, true), (40., false));
    }
    #[test]
    fn reduced_motion_skips_animation() {
        let mut spring = Spring::default();
        spring.step(40., 0.016, true);
        assert_eq!(spring.step(128., 0.016, false), (128., false));
    }
}
