use serde::{Deserialize, Serialize};

use crate::mode::Mode;
use crate::pitch_class::PitchClass;

/// A key: a tonic pitch class plus a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Key {
    pub tonic: PitchClass,
    pub mode: Mode,
}

impl Key {
    pub fn new(tonic: PitchClass, mode: Mode) -> Self {
        Key { tonic, mode }
    }
}
