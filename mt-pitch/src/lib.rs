//! `mt-pitch`: pure pitch detection over `&[f32]` audio buffers, shared by
//! the live trace (data plane) and batch/offline analysis. No I/O, no
//! web-sys, no archive types — samples in, pitch estimates out.

#[cfg(test)]
mod tests {
    #[test]
    fn crate_loads() {
        assert_eq!(2 + 2, 4);
    }
}
