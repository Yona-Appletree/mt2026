//! `mt-archive`: the plain-files practice archive — day-dir WAV + append-only
//! JSONL log (schema v1). Owns the data contract and its (de)serialization;
//! depends on nothing else in this workspace so the format can be read back
//! by tooling that doesn't want the rest of the stack.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }
}
