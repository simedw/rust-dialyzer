//! Folding constructors used by cerl:ann_make_data during shadow renaming.
//! Ordinary source tuple/cons nodes remain structural with the dialyzer option.
use super::{ErlangTerm, Lower, Result, TermId};
impl Lower {
    pub(super) fn ann_c_tuple(&mut self, ann: TermId, elements: Vec<TermId>) -> Result<TermId> {
        if elements
            .iter()
            .any(|&element| self.a.tag(element) != Some("c_literal"))
        {
            let elements = self.a.list(elements);
            return Ok(self.a.tagged("c_tuple", vec![ann, elements]));
        }
        let values = elements
            .into_iter()
            .map(|element| {
                let fields = self.a.tuple_fields(element)?;
                if fields.len() != 3 {
                    return Err("malformed literal tuple element".into());
                }
                Ok(fields[2])
            })
            .collect::<Result<Vec<_>>>()?;
        let value = self.a.tuple(values);
        Ok(self.literal(ann, value))
    }
    pub(super) fn ann_c_cons(&mut self, ann: TermId, head: TermId, tail: TermId) -> Result<TermId> {
        if self.a.tag(head) != Some("c_literal") || self.a.tag(tail) != Some("c_literal") {
            return Ok(self.a.tagged("c_cons", vec![ann, head, tail]));
        }
        let h = self.a.tuple_fields(head)?;
        if h.len() != 3 {
            return Err("malformed literal cons head".into());
        }
        let head = h[2];
        let t = self.a.tuple_fields(tail)?;
        if t.len() != 3 {
            return Err("malformed literal cons tail".into());
        }
        let tail = t[2];
        let value = self.a.push(ErlangTerm::List {
            items: vec![head],
            tail,
        });
        Ok(self.literal(ann, value))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        beam_frontend,
        external_term::{self, Limits},
    };
    #[test]
    fn captured_fun_patterns_fold_but_source_values_and_uncaptured_patterns_do_not() {
        let metadata = beam_frontend::debug::read_beam(
            include_bytes!(
                "../../../tests/fixtures/beam_core_port/literal_shadow/native_literal_shadow.beam"
            ),
            Limits::default(),
        )
        .unwrap();
        let forms = beam_frontend::forms(&metadata).unwrap();
        let raw = beam_frontend::raw_core(&metadata, &forms).unwrap();
        let clean = beam_frontend::clean::clean(&raw).unwrap();
        let expected_raw = external_term::decode(
            include_bytes!("../../../tests/fixtures/beam_core_port/literal_shadow/native_literal_shadow.raw_core.etf"),
            Limits::default()).unwrap();
        let expected_clean = external_term::decode(
            include_bytes!("../../../tests/fixtures/beam_core_port/literal_shadow/native_literal_shadow.clean_core.etf"),
            Limits::default()).unwrap();
        // ETF admits different list segment/string encodings for the same
        // exact Erlang term. Strict term comparison retains float bits and
        // integer/float distinctions without requiring one physical layout.
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
    #[test]
    fn constant_byte_strings_and_exported_regex_blobs_match_otp_core() {
        for (beam, expected_raw, expected_clean) in [
            (
                &include_bytes!(
                    "../../../tests/fixtures/beam_core_port/binary_literals/native_binary_literals.beam"
                )[..],
                &include_bytes!(
                    "../../../tests/fixtures/beam_core_port/binary_literals/native_binary_literals.raw_core.etf"
                )[..],
                &include_bytes!(
                    "../../../tests/fixtures/beam_core_port/binary_literals/native_binary_literals.clean_core.etf"
                )[..],
            ),
            (
                &include_bytes!(
                    "../../../tests/fixtures/beam_core_port/binary_literals/Elixir.NativeRegexBlob.beam"
                )[..],
                &include_bytes!(
                    "../../../tests/fixtures/beam_core_port/binary_literals/Elixir.NativeRegexBlob.raw_core.etf"
                )[..],
                &include_bytes!(
                    "../../../tests/fixtures/beam_core_port/binary_literals/Elixir.NativeRegexBlob.clean_core.etf"
                )[..],
            ),
        ] {
            let metadata = beam_frontend::debug::read_beam(beam, Limits::default()).unwrap();
            let forms = beam_frontend::forms(&metadata).unwrap();
            let raw = beam_frontend::raw_core(&metadata, &forms).unwrap();
            let clean = beam_frontend::clean::clean(&raw).unwrap();
            let expected_raw = external_term::decode(expected_raw, Limits::default()).unwrap();
            let expected_clean = external_term::decode(expected_clean, Limits::default()).unwrap();
            assert_eq!(
                crate::term_order::compare_map_keys(
                    &raw,
                    raw.root,
                    &expected_raw,
                    expected_raw.root
                )
                .unwrap(),
                std::cmp::Ordering::Equal,
                "raw Core {}",
                metadata.module
            );
            assert_eq!(
                crate::term_order::compare_map_keys(
                    &clean,
                    clean.root,
                    &expected_clean,
                    expected_clean.root
                )
                .unwrap(),
                std::cmp::Ordering::Equal,
                "clean Core {}",
                metadata.module
            );
        }
    }
}
