[repo, beam_list, destination] = System.argv()
File.mkdir_p!(destination)
opts = [:no_copt, :to_core, :binary, :return_errors, :no_inline, :strict_record_tests, :dialyzer, :no_spawn_compiler_process]
rows = beam_list |> File.read!() |> String.split("\n",trim: true) |> Enum.with_index() |> Enum.map(fn {beam,index} ->
  {:ok,{name,[debug_info: {:debug_info_v1,backend,data}]}} = :beam_lib.chunks(String.to_charlist(beam),[:debug_info])
  {:ok,core} = backend.debug_info(:core_v1,name,data,opts)
  output=Path.join(destination,"#{index}.clean.etf")
  File.write!(output,:erlang.term_to_binary(:dialyzer_clean_core.clean(core)))
  %{beam: beam, cleaned_core: output, module: Atom.to_string(name)}
end)
File.write!(Path.join(destination,"oracles.json"),:json.encode(rows))
IO.puts(:json.encode(%{modules: length(rows),repo: repo}))
