# Reference only: tests consume saved ETF and never launch this VM.
dir = __DIR__
{:ok, forms} = :epp.parse_file(String.to_charlist(dir <> "/record_probe.erl"), [], [])
prepared = :erl_expand_records.module(forms, [:strict_record_tests, :dialyzer])
File.write!(dir <> "/record_probe.input.etf", :erlang.term_to_binary(forms))
File.write!(dir <> "/record_probe.expected.etf", :erlang.term_to_binary(prepared))
