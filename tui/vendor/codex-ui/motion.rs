// Vendored from openai/codex codex-rs/tui — see UPSTREAM
//
// Copyright OpenAI
// SPDX-License-Identifier: Apache-2.0
//
// Reduced-motion-aware activity indicators (SPEC-109).

use std::time::Instant;

use ratatui::style::Stylize;
use ratatui::text::Span;

use super::shimmer::shimmer_spans;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionMode {
    Animated,
    Reduced,
}

impl MotionMode {
    pub fn from_animations_enabled(animations_enabled: bool) -> Self {
        if animations_enabled {
            Self::Animated
        } else {
            Self::Reduced
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReducedMotionIndicator {
    Hidden,
    StaticBullet,
}

pub fn activity_indicator(
    _start_time: Option<Instant>,
    motion_mode: MotionMode,
    reduced_motion_indicator: ReducedMotionIndicator,
) -> Option<Span<'static>> {
    match motion_mode {
        MotionMode::Animated => shimmer_spans("•").into_iter().next(),
        MotionMode::Reduced => match reduced_motion_indicator {
            ReducedMotionIndicator::Hidden => None,
            ReducedMotionIndicator::StaticBullet => Some("•".dim()),
        },
    }
}

pub fn shimmer_text(text: &str, motion_mode: MotionMode) -> Vec<Span<'static>> {
    match motion_mode {
        MotionMode::Animated => shimmer_spans(text),
        MotionMode::Reduced => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text.to_string().into()]
            }
        }
    }
}
