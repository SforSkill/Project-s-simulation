//! Mouse sensitivity and field of view, compatible with CS:GO conventions.
//!
//! * Yaw: `degrees = counts × sensitivity × m_yaw`, with `m_yaw = 0.022`.
//! * Pitch: same formula with a configurable `m_pitch` (0.022 by default).
//! * No acceleration: the conversion is linear and applied to raw counts, so the
//!   rotation depends only on the distance travelled by the mouse, never on the
//!   frame rate.
//! * FOV: CS:GO defines `fov 90` as the horizontal FOV of a 4:3 image; wider screens
//!   get more horizontal FOV (Hor+), the vertical FOV stays 73.74°.

use serde::{Deserialize, Serialize};

pub const CSGO_M_YAW: f32 = 0.022;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sensitivity {
    pub sensitivity: f32,
    pub m_yaw: f32,
    pub m_pitch: f32,
    /// CS:GO `zoom_sensitivity_ratio_mouse`.
    pub zoom_ratio: f32,
    pub invert_y: bool,
}

impl Default for Sensitivity {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            m_yaw: CSGO_M_YAW,
            m_pitch: CSGO_M_YAW,
            zoom_ratio: 1.0,
            invert_y: false,
        }
    }
}

impl Sensitivity {
    /// Converts raw mouse counts to (pitch, yaw) deltas in degrees.
    /// `zoom_fov_ratio` is `zoomed_fov / default_fov` (1.0 when not scoped).
    pub fn counts_to_degrees(&self, dx: f64, dy: f64, zoom_fov_ratio: f32) -> (f32, f32) {
        let zoom = if zoom_fov_ratio < 1.0 {
            zoom_fov_ratio * self.zoom_ratio
        } else {
            1.0
        };
        let s = self.sensitivity as f64 * zoom as f64;
        let yaw = -(dx * s * self.m_yaw as f64);
        let sign = if self.invert_y { 1.0 } else { -1.0 };
        let pitch = sign * dy * s * self.m_pitch as f64;
        (pitch as f32, yaw as f32)
    }

    /// Mouse distance in centimetres for a full 360° turn.
    pub fn cm_per_360(&self, dpi: f32) -> f32 {
        let counts = 360.0 / (self.sensitivity * self.m_yaw);
        counts / dpi * 2.54
    }
}

/// CS:GO reference: horizontal FOV at 4:3.
pub const CSGO_FOV_4_3: f32 = 90.0;

/// Vertical FOV (degrees) corresponding to a horizontal FOV defined at 4:3.
pub fn vertical_fov_from_fov43(fov43_deg: f32) -> f32 {
    let h = (fov43_deg.to_radians() * 0.5).tan();
    (2.0 * (h * 3.0 / 4.0).atan()).to_degrees()
}

/// Horizontal FOV on a screen of the given aspect ratio (Hor+).
pub fn horizontal_fov(fov43_deg: f32, aspect: f32) -> f32 {
    let v = vertical_fov_from_fov43(fov43_deg).to_radians();
    (2.0 * ((v * 0.5).tan() * aspect).atan()).to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_turn_distance_matches_csgo() {
        // 800 DPI, sensitivity 1, m_yaw 0.022 → 360 / 0.022 = 16363.6 counts
        // = 20.45 inches = 51.95 cm (value given in the design document).
        let s = Sensitivity::default();
        let cm = s.cm_per_360(800.0);
        assert!((cm - 51.95).abs() < 0.01, "{cm}");
        // Check the conversion itself: 16363.636 counts give exactly 360°.
        let (_, yaw) = s.counts_to_degrees(-(360.0 / 0.022), 0.0, 1.0);
        assert!((yaw - 360.0).abs() < 1e-3);
    }

    #[test]
    fn rotation_is_independent_of_frame_splitting() {
        // The same mouse travel split in 1, 7 or 1000 frames gives the same angle.
        let s = Sensitivity {
            sensitivity: 2.3,
            ..Default::default()
        };
        let total = 5000.0;
        for frames in [1usize, 7, 1000] {
            let mut yaw = 0.0f64;
            for _ in 0..frames {
                yaw += s.counts_to_degrees(total / frames as f64, 0.0, 1.0).1 as f64;
            }
            assert!(
                (yaw + total * 2.3 * 0.022).abs() < 1e-3,
                "{frames} frames: {yaw}"
            );
        }
    }

    #[test]
    fn fov_conventions() {
        assert!((vertical_fov_from_fov43(90.0) - 73.7398).abs() < 1e-3);
        assert!((horizontal_fov(90.0, 4.0 / 3.0) - 90.0).abs() < 1e-3);
        assert!((horizontal_fov(90.0, 16.0 / 9.0) - 106.26).abs() < 0.01);
    }

    #[test]
    fn zoom_scales_sensitivity() {
        let s = Sensitivity::default();
        let (_, full) = s.counts_to_degrees(100.0, 0.0, 1.0);
        let (_, zoomed) = s.counts_to_degrees(100.0, 0.0, 40.0 / 90.0);
        assert!((zoomed / full - 40.0 / 90.0).abs() < 1e-5);
    }
}
