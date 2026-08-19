use serde::{Deserialize, Serialize};

/// A scale mode. Only the two v0.1 needs; more (dorian, etc.) can join
/// later without breaking this enum's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Major,
    Minor,
}

impl Mode {
    /// Semitone offsets from the tonic for scale degrees 1..=7, in order.
    ///
    /// Major: the familiar W-W-H-W-W-W-H pattern. Minor: natural minor.
    pub(crate) fn degree_intervals(self) -> [u8; 7] {
        match self {
            Mode::Major => [0, 2, 4, 5, 7, 9, 11],
            Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
        }
    }
}
