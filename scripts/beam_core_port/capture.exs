opts = [:no_copt, :to_core, :binary, :return_errors, :no_inline, :strict_record_tests, :dialyzer, :no_spawn_compiler_process]
[destination | beams] = System.argv()
File.mkdir_p!(destination)
for beam <- beams do
  {:ok, {module, [debug_info: debug = {:debug_info_v1, backend, data}]}} = :beam_lib.chunks(String.to_charlist(beam), [:debug_info])
  {:ok, forms} = backend.debug_info(:erlang_v1, module, data, opts)
  {:ok, raw} = backend.debug_info(:core_v1, module, data, opts)
  clean = :dialyzer_clean_core.clean(raw)
  for {stage, term} <- [debug: debug, forms: forms, raw_core: raw, clean_core: clean] do
    File.write!("#{destination}/#{module}.#{stage}.etf", :erlang.term_to_binary(term))
    File.write!("#{destination}/#{module}.#{stage}.txt", :io_lib.format(~c"~p.~n", [term]))
  end
end
