//! `mt-session`: the humble-view drill state machine — events in, state and
//! commands out. Side effects (audio out, capture, storage) live behind port
//! traits so this crate is unit-testable with fakes; it never touches a
//! browser or a web framework directly.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }
}
