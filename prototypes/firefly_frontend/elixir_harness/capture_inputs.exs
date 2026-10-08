# Oracle preparation only. Native executables never invoke this script.
[root, out] = System.argv()
paths = Path.wildcard(Path.join(root, "examples/tiny/_build/dev/lib/tiny/ebin/*.beam")) ++
  Path.wildcard(Path.join(root, ".cache/corpus/elixir/_elixir/compiled/*.beam")) ++
  Path.wildcard(Path.join(root, ".cache/corpus/elixir/dialyxir__examples/beams/*.beam"))
rows = for beam <- paths do
  {:ok, {name, [debug_info: debug = {:debug_info_v1, backend, metadata}]}} = :beam_lib.chunks(String.to_charlist(beam), [:debug_info])
  {:ok, forms} = backend.debug_info(:erlang_v1, name, metadata, [])
  dir = Path.join(out, Atom.to_string(name))
  File.mkdir_p!(dir)
  File.write!(Path.join(dir,"debug-info.etf"), :erlang.term_to_binary(debug))
  File.write!(Path.join(dir,"metadata.etf"), :erlang.term_to_binary(metadata))
  File.write!(Path.join(dir,"oracle-source-forms.etf"), :erlang.term_to_binary(forms))
  File.write!(Path.join(dir,"debug-info.txt"), inspect(debug, pretty: true, limit: :infinity, printable_limit: :infinity, width: 110))
  File.write!(Path.join(dir,"oracle-source-forms.txt"), inspect(forms, pretty: true, limit: :infinity, printable_limit: :infinity, width: 110))
  %{module: Atom.to_string(name), beam_path: beam, beam_sha256: Base.encode16(:crypto.hash(:sha256, File.read!(beam)),case: :lower), directory: dir}
end
File.write!(Path.join(out,"inputs.json"), :json.encode(%{oracle_only: true, original_beams_not_recompiled: true, cases: rows}))
IO.inspect(%{modules: length(rows), output: out})
