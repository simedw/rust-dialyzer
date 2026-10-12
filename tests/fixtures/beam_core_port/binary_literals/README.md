# Binary literal and exported Regex oracle fixtures

Captured independently with Elixir 1.19.4 and OTP 28.3 using `scripts/beam_core_port/capture.exs`. The test compares the complete native raw and cleaned Core terms against these compiler outputs.

`native_binary_literals.erl` covers large zero-filled and mixed byte strings, a wide original integer field whose padding cost deliberately prevents literal folding, empty strings, and dynamic binaries after strings of exactly 128 and 256 bytes. OTP emits a trailing zero-width integer segment after each exact 1024-bit string boundary; the fixture preserves that structural detail even though it does not change the resulting binary bytes.

`native_regex_blob.ex` contains literal Regex values that the Elixir debug backend lifts into `re:import/1` calls with serialized byte strings. Original string fields have 8-bit elements; the later synthetic 1024-bit chunks must not trigger the wide original-integer padding heuristic.

Regenerate by compiling the Erlang source with `erlc +debug_info`, compiling the Elixir source with `elixirc`, and running the capture script on both resulting BEAM files. Production tests read the saved fixtures and invoke no VM or compiler.
