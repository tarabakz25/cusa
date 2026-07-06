// Copyright 2026 cusa contributors
// SPDX-License-Identifier: Apache-2.0
//
// Stand-in for upstream `terminal_probe` used by vendored `terminal_palette`
// during P0. Returns `None` so palette code falls back to ANSI defaults until
// a bounded OSC 10/11 probe is ported (SPEC-105).

use std::io;
use std::time::Duration;

pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) struct DefaultColors {
    pub(crate) fg: (u8, u8, u8),
    pub(crate) bg: (u8, u8, u8),
}

/// P0 stub: skip blocking terminal color probes during foundation integration.
pub(crate) fn default_colors(
    _timeout: Duration,
) -> io::Result<Option<DefaultColors>> {
    Ok(None)
}
