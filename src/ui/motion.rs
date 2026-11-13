//! Pure motion math: spring easing for entrances and the turntable spin model.

use std::time::{Duration, Instant};

use super::theme;

/// Spring position for a linear `gpui::Animation` delta. Not usable as an easing: GPUI
/// asserts easings stay in 0..=1, and the spring overshoots, so animators call this instead.
pub fn spring_progress(linear_delta: f32, duration: Duration) -> f32 {
    spring_position(linear_delta * duration.as_secs_f32())
}

/// Position of a unit-step damped spring after `seconds`, starting at 0 at rest.
pub fn spring_position(seconds: f32) -> f32 {
    let natural = theme::SPRING_FREQUENCY_HZ * std::f32::consts::TAU;
    let damping = theme::SPRING_DAMPING_RATIO;
    let damped = natural * (1.0 - damping * damping).sqrt();
    let decay = (-damping * natural * seconds).exp();
    1.0 - decay * ((damped * seconds).cos() + damping * natural / damped * (damped * seconds).sin())
}

/// A turntable whose angular velocity relaxes exponentially toward a target
/// (motor spin-up, or friction braking when paused). Angle is integrated analytically,
/// so it is frame-rate independent.
#[derive(Debug, Clone, Copy)]
pub struct VinylSpin {
    anchor_time: Instant,
    anchor_angle: f32,
    anchor_velocity: f32,
    target_velocity: f32,
    time_constant_seconds: f32,
}

const RESTING_VELOCITY_EPSILON: f32 = 0.01;

impl VinylSpin {
    pub fn at_rest(now: Instant) -> Self {
        Self {
            anchor_time: now,
            anchor_angle: 0.0,
            anchor_velocity: 0.0,
            target_velocity: 0.0,
            time_constant_seconds: theme::VINYL_SPIN_DOWN_SECONDS,
        }
    }

    pub fn set_playing(&mut self, is_playing: bool, now: Instant) {
        let target = if is_playing {
            theme::VINYL_PLAYING_RADIANS_PER_SECOND
        } else {
            0.0
        };
        if target == self.target_velocity {
            return;
        }
        self.anchor_angle = self.angle_at(now);
        self.anchor_velocity = self.velocity_at(now);
        self.anchor_time = now;
        self.target_velocity = target;
        self.time_constant_seconds = if is_playing {
            theme::VINYL_SPIN_UP_SECONDS
        } else {
            theme::VINYL_SPIN_DOWN_SECONDS
        };
    }

    pub fn velocity_at(&self, now: Instant) -> f32 {
        let elapsed = self.elapsed_seconds(now);
        self.target_velocity
            + (self.anchor_velocity - self.target_velocity)
                * (-elapsed / self.time_constant_seconds).exp()
    }

    /// Radians, wrapped to one turn.
    pub fn angle_at(&self, now: Instant) -> f32 {
        let elapsed = self.elapsed_seconds(now);
        let settled = (-elapsed / self.time_constant_seconds).exp();
        let angle = self.anchor_angle
            + self.target_velocity * elapsed
            + (self.anchor_velocity - self.target_velocity)
                * self.time_constant_seconds
                * (1.0 - settled);
        angle.rem_euclid(std::f32::consts::TAU)
    }

    /// True while the disc visibly moves; the view requests frames only then.
    pub fn is_moving(&self, now: Instant) -> bool {
        self.target_velocity != 0.0 || self.velocity_at(now).abs() > RESTING_VELOCITY_EPSILON
    }

    fn elapsed_seconds(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.anchor_time)
            .as_secs_f32()
    }
}

/// Moves `shown` toward `target` with an exponential follow; returns the new value.
pub fn smooth_toward(shown: f32, target: f32, elapsed: Duration) -> f32 {
    let blend = 1.0 - (-elapsed.as_secs_f32() / theme::PROGRESS_SMOOTHING_SECONDS).exp();
    let next = shown + (target - shown) * blend;
    if (target - next).abs() < 0.001 {
        target
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_starts_at_zero_overshoots_and_settles() {
        assert!(spring_position(0.0).abs() < 1e-6);
        let peak = (1..200)
            .map(|step| spring_position(step as f32 / 200.0))
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.0 && peak < 1.25, "peak {peak}");
        assert!((spring_position(2.0) - 1.0).abs() < 0.01);
    }

    #[test]
    fn vinyl_spins_up_then_brakes_to_rest() {
        let start = Instant::now();
        let mut spin = VinylSpin::at_rest(start);
        assert!(!spin.is_moving(start));
        spin.set_playing(true, start);
        let later = start + Duration::from_secs(3);
        assert!((spin.velocity_at(later) - theme::VINYL_PLAYING_RADIANS_PER_SECOND).abs() < 0.05);
        spin.set_playing(false, later);
        assert!(spin.is_moving(later + Duration::from_millis(500)));
        assert!(!spin.is_moving(later + Duration::from_secs(10)));
        let resting_angle = spin.angle_at(later + Duration::from_secs(10));
        assert!((spin.angle_at(later + Duration::from_secs(20)) - resting_angle).abs() < 1e-3);
    }

    #[test]
    fn smoothing_converges_without_overshoot() {
        let mut shown = 0.0;
        for _ in 0..120 {
            shown = smooth_toward(shown, 0.6, Duration::from_millis(16));
            assert!(shown <= 0.6);
        }
        assert_eq!(shown, 0.6);
    }
}
