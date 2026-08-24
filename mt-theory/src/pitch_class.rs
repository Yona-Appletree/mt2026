use serde::{Deserialize, Serialize};

/// One of the twelve equal-temperament pitch classes, spelled with sharps.
///
/// v0.1 only needs one spelling per semitone (enharmonic equivalents like
/// `D#`/`Eb` are not modeled separately) — good enough for movable-do drills
/// and frequency math, not for notation-accurate key spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PitchClass {
    #[serde(rename = "C")]
    C,
    #[serde(rename = "C#")]
    CSharp,
    #[serde(rename = "D")]
    D,
    #[serde(rename = "D#")]
    DSharp,
    #[serde(rename = "E")]
    E,
    #[serde(rename = "F")]
    F,
    #[serde(rename = "F#")]
    FSharp,
    #[serde(rename = "G")]
    G,
    #[serde(rename = "G#")]
    GSharp,
    #[serde(rename = "A")]
    A,
    #[serde(rename = "A#")]
    ASharp,
    #[serde(rename = "B")]
    B,
}

impl PitchClass {
    /// All twelve pitch classes in ascending chromatic order starting at C.
    pub const ALL: [PitchClass; 12] = [
        PitchClass::C,
        PitchClass::CSharp,
        PitchClass::D,
        PitchClass::DSharp,
        PitchClass::E,
        PitchClass::F,
        PitchClass::FSharp,
        PitchClass::G,
        PitchClass::GSharp,
        PitchClass::A,
        PitchClass::ASharp,
        PitchClass::B,
    ];

    /// Semitone offset from C (C=0 .. B=11), i.e. this pitch class's index
    /// within an octave under standard scientific pitch notation.
    pub fn semitone_from_c(self) -> u8 {
        self as u8
    }

    /// The pitch class `semitone` steps above C (mod 12).
    pub fn from_semitone(semitone: u8) -> PitchClass {
        PitchClass::ALL[(semitone % 12) as usize]
    }
}
