use crate::degree::Degree;
use crate::key::Key;
use crate::pitch_class::PitchClass;

/// Reference pitch: A4 = 440.0 Hz (equal temperament).
pub const A4_HZ: f64 = 440.0;

/// A4's MIDI-style note number under standard scientific pitch notation
/// (octave increments at C; C4 = 60, A4 = 69).
const A4_NOTE_NUMBER: i32 = 69;

/// Absolute note number for a pitch class in a given octave, using the same
/// numbering MIDI uses (C4 = 60, so A4 = 69).
fn note_number(pitch_class: PitchClass, octave: i32) -> i32 {
    12 * (octave + 1) + pitch_class.semitone_from_c() as i32
}

/// Inverse of [`note_number`]: the (pitch class, octave) an absolute note
/// number denotes.
fn from_note_number(note_number: i32) -> (PitchClass, i32) {
    let octave = note_number.div_euclid(12) - 1;
    let semitone = note_number.rem_euclid(12) as u8;
    (PitchClass::from_semitone(semitone), octave)
}

/// Equal-temperament frequency of an absolute note number, referenced to
/// A4 = 440.0 Hz.
fn note_number_to_hz(note_number: i32) -> f64 {
    A4_HZ * 2f64.powf((note_number - A4_NOTE_NUMBER) as f64 / 12.0)
}

/// The semitone offset of `degree` above the key's tonic, per the key's mode.
fn degree_interval_semitones(key: Key, degree: Degree) -> i32 {
    key.mode.degree_intervals()[(degree.get() - 1) as usize] as i32
}

/// Octave-placement helper: given a reference octave for the tonic (e.g. D4
/// is `tonic_octave = 4`), returns the (pitch class, octave) a scale degree
/// lands on, using standard scientific-pitch-notation octave numbering
/// (the octave increments at C, not at the tonic) — e.g. the 7th degree of D
/// major starting at D4 is C#5, not C#4, because C# falls after the C
/// octave boundary.
pub fn place_degree(key: Key, tonic_octave: i32, degree: Degree) -> (PitchClass, i32) {
    let tonic_note = note_number(key.tonic, tonic_octave);
    let target_note = tonic_note + degree_interval_semitones(key, degree);
    from_note_number(target_note)
}

/// Equal-temperament frequency (Hz, A4 = 440.0) of a scale degree in `key`,
/// with the tonic placed at `tonic_octave` (e.g. `degree_to_hz(d_major, sol,
/// 4)` is the fifth degree of D major starting from D4).
pub fn degree_to_hz(key: Key, degree: Degree, tonic_octave: i32) -> f64 {
    let tonic_note = note_number(key.tonic, tonic_octave);
    let target_note = tonic_note + degree_interval_semitones(key, degree);
    note_number_to_hz(target_note)
}

/// Signed distance in cents between two frequencies: `1200 * log2(a / b)`.
/// Positive when `a` is sharper than `b`.
pub fn cents_between(a_hz: f64, b_hz: f64) -> f64 {
    1200.0 * (a_hz / b_hz).log2()
}

/// Reference frequencies for the do-mi-sol-do establishment arpeggio (Q13):
/// degrees 1, 3, 5 in `tonic_octave`, then degree 1 an octave up.
pub fn arpeggio_do_mi_sol_do(key: Key, tonic_octave: i32) -> [f64; 4] {
    let do_low = Degree::new(1).expect("1 is a valid degree");
    let mi = Degree::new(3).expect("3 is a valid degree");
    let sol = Degree::new(5).expect("5 is a valid degree");
    [
        degree_to_hz(key, do_low, tonic_octave),
        degree_to_hz(key, mi, tonic_octave),
        degree_to_hz(key, sol, tonic_octave),
        degree_to_hz(key, do_low, tonic_octave + 1),
    ]
}
