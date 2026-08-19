use serde::{Deserialize, Serialize};

/// A scale degree, 1..=7 (movable-do). Constructed via [`Degree::new`] so an
/// out-of-range value can never exist.
///
/// Serializes as a bare integer (it's a newtype/transparent struct), matching
/// how degrees appear as plain numbers in the archive's JSONL schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Degree(u8);

impl Degree {
    /// Builds a degree from a 1..=7 scale position. Returns `None` outside
    /// that range.
    pub fn new(n: u8) -> Option<Self> {
        if (1..=7).contains(&n) {
            Some(Degree(n))
        } else {
            None
        }
    }

    /// The underlying 1..=7 scale position.
    pub fn get(self) -> u8 {
        self.0
    }

    /// Movable-do solfège name for this degree.
    ///
    /// **Boundary (v0.1):** this always returns the *major*-scale names
    /// (do re mi fa sol la ti) regardless of the key's mode — minor-mode
    /// solfège (e.g. lowered `me`/`le`/`te`) is not modeled yet. Callers
    /// pairing this with a minor [`crate::Key`] get major-scale syllables
    /// naming degrees whose pitch (via [`crate::degree_to_hz`]) is actually
    /// the natural-minor interval.
    pub fn solfege_name(self) -> &'static str {
        match self.0 {
            1 => "do",
            2 => "re",
            3 => "mi",
            4 => "fa",
            5 => "sol",
            6 => "la",
            7 => "ti",
            _ => unreachable!("Degree is constructed only via new(), which validates 1..=7"),
        }
    }
}
