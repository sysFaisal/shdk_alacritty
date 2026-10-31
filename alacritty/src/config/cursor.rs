use std::cmp;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use alacritty_config_derive::{ConfigDeserialize, SerdeReplace};
use alacritty_terminal::vte::ansi::{CursorShape as VteCursorShape, CursorStyle as VteCursorStyle};

use crate::config::ui_config::Percentage;

/// The minimum blink interval value in milliseconds.
const MIN_BLINK_INTERVAL: u64 = 10;

/// The minimum number of blinks before pausing.
const MIN_BLINK_CYCLES_BEFORE_PAUSE: u64 = 1;

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Debug, PartialEq)]
pub struct Cursor {
    pub style: ConfigCursorStyle,
    pub vi_mode_style: Option<ConfigCursorStyle>,
    pub unfocused_hollow: bool,

    thickness: Percentage,
    blink_interval: u64,
    blink_timeout: u8,

    /// GPU cursor trail configuration.
    pub trail: CursorTrail,
}

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Debug, PartialEq)]
pub struct CursorTrail {
    /// Enable the GPU cursor trail.
    pub enabled: bool,
    /// Fast and slow decay in milliseconds.
    pub decay: CursorTrailDecay,
    /// Horizontal and vertical movement required to start a trail, in cells.
    pub start_threshold: CursorTrailThreshold,
}

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Debug, PartialEq, Default)]
pub struct CursorTrailDecay {
    pub fast: u32,
    pub slow: u32,
}

#[derive(ConfigDeserialize, Serialize, Copy, Clone, Debug, PartialEq, Default)]
pub struct CursorTrailThreshold {
    pub horizontal: u32,
    pub vertical: u32,
}

impl CursorTrail {
    pub fn slow_decay(self) -> u64 {
        u64::from(self.decay.slow.max(self.decay.fast))
    }

    pub fn threshold(self) -> (u32, u32) {
        (self.start_threshold.horizontal, self.start_threshold.vertical)
    }
}

impl Default for CursorTrail {
    fn default() -> Self {
        Self {
            enabled: false,
            decay: CursorTrailDecay { fast: 100, slow: 400 },
            start_threshold: CursorTrailThreshold { horizontal: 2, vertical: 2 },
        }
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            thickness: Percentage::new(0.15),
            unfocused_hollow: true,
            blink_interval: 750,
            blink_timeout: 5,
            style: Default::default(),
            vi_mode_style: Default::default(),
            trail: Default::default(),
        }
    }
}

impl Cursor {
    #[inline]
    pub fn thickness(self) -> f32 {
        self.thickness.as_f32()
    }

    #[inline]
    pub fn style(self) -> VteCursorStyle {
        self.style.into()
    }

    #[inline]
    pub fn vi_mode_style(self) -> Option<VteCursorStyle> {
        self.vi_mode_style.map(Into::into)
    }

    #[inline]
    pub fn blink_interval(self) -> u64 {
        cmp::max(self.blink_interval, MIN_BLINK_INTERVAL)
    }

    #[inline]
    pub fn blink_timeout(self) -> Duration {
        if self.blink_timeout == 0 {
            Duration::ZERO
        } else {
            cmp::max(
                // Show/hide is what we consider a cycle, so multiply by `2`.
                Duration::from_millis(self.blink_interval * 2 * MIN_BLINK_CYCLES_BEFORE_PAUSE),
                Duration::from_secs(self.blink_timeout as u64),
            )
        }
    }
}

#[derive(SerdeReplace, Deserialize, Serialize, Debug, Copy, Clone, PartialEq, Eq)]
#[serde(untagged, deny_unknown_fields)]
pub enum ConfigCursorStyle {
    Shape(CursorShape),
    WithBlinking {
        #[serde(default)]
        shape: CursorShape,
        #[serde(default)]
        blinking: CursorBlinking,
    },
}

impl Default for ConfigCursorStyle {
    fn default() -> Self {
        Self::Shape(CursorShape::default())
    }
}

impl ConfigCursorStyle {
    /// Check if blinking is force enabled/disabled.
    pub fn blinking_override(&self) -> Option<bool> {
        match self {
            Self::Shape(_) => None,
            Self::WithBlinking { blinking, .. } => blinking.blinking_override(),
        }
    }
}

impl From<ConfigCursorStyle> for VteCursorStyle {
    fn from(config_style: ConfigCursorStyle) -> Self {
        match config_style {
            ConfigCursorStyle::Shape(shape) => Self { shape: shape.into(), blinking: false },
            ConfigCursorStyle::WithBlinking { shape, blinking } => {
                Self { shape: shape.into(), blinking: blinking.into() }
            },
        }
    }
}

#[derive(ConfigDeserialize, Serialize, Default, Debug, Copy, Clone, PartialEq, Eq)]
pub enum CursorBlinking {
    Never,
    #[default]
    Off,
    On,
    Always,
}

impl CursorBlinking {
    fn blinking_override(&self) -> Option<bool> {
        match self {
            Self::Never => Some(false),
            Self::Off | Self::On => None,
            Self::Always => Some(true),
        }
    }
}

impl From<CursorBlinking> for bool {
    fn from(blinking: CursorBlinking) -> bool {
        blinking == CursorBlinking::On || blinking == CursorBlinking::Always
    }
}

#[derive(ConfigDeserialize, Serialize, Debug, Default, Eq, PartialEq, Copy, Clone, Hash)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Beam,
}

impl From<CursorShape> for VteCursorShape {
    fn from(value: CursorShape) -> Self {
        match value {
            CursorShape::Block => VteCursorShape::Block,
            CursorShape::Underline => VteCursorShape::Underline,
            CursorShape::Beam => VteCursorShape::Beam,
        }
    }
}
