//! Independent OTP 28.3 record-preparation golden; no compiler runtime required.
use rust_dialyzer_poc::{beam_frontend::erl_prepare, external_term};

#[test]
fn otp_record_preparation_preserves_complete_forms() {
    let limits = external_term::Limits::default();
    let input = external_term::decode(
        include_bytes!("fixtures/beam_core_port/record_preparation/record_probe.input.etf"),
        limits,
    )
    .unwrap();
    let expected = external_term::decode(
        include_bytes!("fixtures/beam_core_port/record_preparation/record_probe.expected.etf"),
        limits,
    )
    .unwrap();
    let actual = erl_prepare::prepare(&input).unwrap();
    assert_eq!(
        external_term::encode(&actual, limits, false).unwrap(),
        external_term::encode(&expected, limits, false).unwrap(),
    );
}
