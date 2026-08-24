//! `mt-theory`: pure music-theory value types and math (scales, degrees,
//! intervals, frequency conversions). No I/O, no dependencies on any other
//! crate in this workspace — values in, values out.
//!
//! Covers what the v0.1 degree drill needs: a [`Key`] (tonic + [`Mode`]),
//! [`Degree`] (1..=7, movable-do), equal-temperament frequency lookup
//! ([`degree_to_hz`]) referenced to A4 = 440.0 Hz, octave placement
//! ([`place_degree`]), cents comparison ([`cents_between`]), and the
//! do-mi-sol-do establishment arpeggio ([`arpeggio_do_mi_sol_do`]).
//!
//! Minor-mode solfège is a known v0.1 gap — see [`Degree::solfege_name`].

mod degree;
mod frequency;
mod key;
mod mode;
mod pitch_class;

pub use degree::Degree;
pub use frequency::{A4_HZ, arpeggio_do_mi_sol_do, cents_between, degree_to_hz, place_degree};
pub use key::Key;
pub use mode::Mode;
pub use pitch_class::PitchClass;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }

    #[test]
    fn sol_in_d_major_from_d4_is_a4_440() {
        let key = Key::new(PitchClass::D, Mode::Major);
        let sol = Degree::new(5).unwrap();
        let hz = degree_to_hz(key, sol, 4);
        assert!((hz - 440.0).abs() < 0.01, "expected ~440.0, got {hz}");
    }

    #[test]
    fn tonic_d4_is_293_66() {
        let key = Key::new(PitchClass::D, Mode::Major);
        let tonic = Degree::new(1).unwrap();
        let hz = degree_to_hz(key, tonic, 4);
        assert!((hz - 293.66).abs() < 0.01, "expected ~293.66, got {hz}");
    }

    #[test]
    fn seventh_degree_of_d_major_crosses_into_octave_5() {
        // D major's 7th degree is C#, which under scientific pitch notation
        // (octave increments at C, not at the tonic) sits in octave 5 when
        // the tonic is D4, not octave 4.
        let key = Key::new(PitchClass::D, Mode::Major);
        let seventh = Degree::new(7).unwrap();
        let (pitch_class, octave) = place_degree(key, 4, seventh);
        assert_eq!(pitch_class, PitchClass::CSharp);
        assert_eq!(octave, 5);
    }

    #[test]
    fn cents_between_is_signed_and_matches_a_semitone() {
        // A4 (440) vs. Ab4 (415.30) is one semitone ≈ 100 cents, positive
        // because 440 is sharper than 415.30.
        let cents = cents_between(440.0, 415.30);
        assert!((cents - 100.0).abs() < 0.1, "expected ~+100, got {cents}");

        // Reversing the operands flips the sign.
        let reversed = cents_between(415.30, 440.0);
        assert!(
            (reversed + 100.0).abs() < 0.1,
            "expected ~-100, got {reversed}"
        );
    }

    #[test]
    fn cents_between_identical_frequencies_is_zero() {
        assert_eq!(cents_between(440.0, 440.0), 0.0);
    }

    #[test]
    fn degree_names_are_movable_do_major_solfege() {
        let names: Vec<&str> = (1..=7)
            .map(|n| Degree::new(n).unwrap().solfege_name())
            .collect();
        assert_eq!(names, ["do", "re", "mi", "fa", "sol", "la", "ti"]);
    }

    #[test]
    fn degree_new_rejects_out_of_range() {
        assert!(Degree::new(0).is_none());
        assert!(Degree::new(8).is_none());
        assert!(Degree::new(1).is_some());
        assert!(Degree::new(7).is_some());
    }

    #[test]
    fn arpeggio_do_mi_sol_do_matches_expected_frequencies() {
        let key = Key::new(PitchClass::D, Mode::Major);
        let freqs = arpeggio_do_mi_sol_do(key, 4);

        let expected_do = degree_to_hz(key, Degree::new(1).unwrap(), 4);
        let expected_mi = degree_to_hz(key, Degree::new(3).unwrap(), 4);
        let expected_sol = degree_to_hz(key, Degree::new(5).unwrap(), 4);
        let expected_do_up = degree_to_hz(key, Degree::new(1).unwrap(), 5);

        assert_eq!(
            freqs,
            [expected_do, expected_mi, expected_sol, expected_do_up]
        );
        // sol matches the other test's reference: A4 = 440.0
        assert!((freqs[2] - 440.0).abs() < 0.01);
        // the closing do is exactly an octave above the opening do
        assert!((freqs[3] - freqs[0] * 2.0).abs() < 1e-9);
    }

    #[test]
    fn pitch_class_round_trips_through_json() {
        for pc in PitchClass::ALL {
            let json = serde_json::to_string(&pc).unwrap();
            let back: PitchClass = serde_json::from_str(&json).unwrap();
            assert_eq!(pc, back);
        }
    }

    #[test]
    fn key_round_trips_through_json() {
        let key = Key::new(PitchClass::D, Mode::Major);
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, r#"{"tonic":"D","mode":"major"}"#);
        let back: Key = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn degree_round_trips_through_json_as_bare_integer() {
        let degree = Degree::new(5).unwrap();
        let json = serde_json::to_string(&degree).unwrap();
        assert_eq!(json, "5");
        let back: Degree = serde_json::from_str(&json).unwrap();
        assert_eq!(degree, back);
    }

    #[test]
    fn mode_round_trips_through_json() {
        for mode in [Mode::Major, Mode::Minor] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: Mode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }
}
