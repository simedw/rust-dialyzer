//! Independent OTP oracle checks for record guards and pinned-guard sequencing.
#[cfg(test)]
mod tests {
    use crate::{
        beam_frontend,
        external_term::{self, Limits},
    };
    #[test]
    fn record_guards_and_pinned_guard_prefix_match_otp_core() {
        let metadata = beam_frontend::debug::read_beam(
            include_bytes!(
                "../../../tests/fixtures/beam_core_port/record_guards/native_record_guards.beam"
            ),
            Limits::default(),
        )
        .unwrap();
        let forms = beam_frontend::forms(&metadata).unwrap();
        let raw = beam_frontend::raw_core(&metadata, &forms).unwrap();
        let clean = beam_frontend::clean::clean(&raw).unwrap();
        let expected_raw = external_term::decode(
            include_bytes!("../../../tests/fixtures/beam_core_port/record_guards/native_record_guards.raw_core.etf"),
            Limits::default()).unwrap();
        let expected_clean = external_term::decode(
            include_bytes!("../../../tests/fixtures/beam_core_port/record_guards/native_record_guards.clean_core.etf"),
            Limits::default()).unwrap();
        assert_eq!(
            crate::term_order::compare_map_keys(&raw, raw.root, &expected_raw, expected_raw.root)
                .unwrap(),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            crate::term_order::compare_map_keys(
                &clean,
                clean.root,
                &expected_clean,
                expected_clean.root
            )
            .unwrap(),
            std::cmp::Ordering::Equal
        );
    }
}
