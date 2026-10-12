# Oracle capture only. Rust integration tests read the saved artifacts.
for name <- ["native_comprehension", "native_comprehension_edges", "native_comprehension_more", "native_comprehension_scope", "native_zip_patterns", "native_generator_rhs", "native_strict_wildcards"] do
  dir = __DIR__
  {:ok, module} = :compile.file(String.to_charlist(dir <> "/#{name}.erl"), [:debug_info, {:outdir, String.to_charlist(dir)}])
  {:ok, {^module, [debug_info: {:debug_info_v1, backend, metadata}]}} = :beam_lib.chunks(String.to_charlist(dir <> "/#{name}.beam"), [:debug_info])
  opts = [:no_copt, :to_core, :binary, :return_errors, :no_inline, :strict_record_tests, :dialyzer, :no_spawn_compiler_process]
  {:ok, forms} = backend.debug_info(:erlang_v1, module, metadata, opts)
  {:ok, core} = backend.debug_info(:core_v1, module, metadata, opts)
  File.write!(dir <> "/#{name}.forms.etf", :erlang.term_to_binary(forms))
  File.write!(dir <> "/#{name}.raw_core.etf", :erlang.term_to_binary(core))
end
