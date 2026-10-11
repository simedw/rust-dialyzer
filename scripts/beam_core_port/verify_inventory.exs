Code.require_file("compare_terms.exs", __DIR__)
# Oracle-only corpus verification. Native output files come from the Rust binary.
[plan, result_path] = System.argv()
opts = [:no_copt, :to_core, :binary, :return_errors, :no_inline, :strict_record_tests, :dialyzer, :no_spawn_compiler_process]
results = for line <- File.stream!(plan), reduce: [] do
  acc ->
    [beam,stage,actual_path] = line |> String.trim_trailing() |> String.split("\t")
    result = try do
      {:ok,{module,[debug_info: {:debug_info_v1,backend,data}]}} = :beam_lib.chunks(String.to_charlist(beam),[:debug_info])
      {:ok,expected} = case stage do
        "forms" -> backend.debug_info(:erlang_v1,module,data,opts)
        "core" -> backend.debug_info(:core_v1,module,data,opts)
        "clean" -> with {:ok,core} <- backend.debug_info(:core_v1,module,data,opts), do: {:ok,:dialyzer_clean_core.clean(core)}
      end
      actual = actual_path |> File.read!() |> :erlang.binary_to_term()
      row = %{"beam"=>beam,"stage"=>stage,"equal"=>actual === expected,"module"=>Atom.to_string(module)}
      case ExactTermComparison.difference(expected,actual,"root") do
        nil -> row
        {path,a,b} ->
          File.write!(actual_path <> ".oracle.etf", :erlang.term_to_binary(expected))
          Map.merge(row,%{"difference_path"=>path,"expected"=>inspect(a,limit: 8),"actual"=>inspect(b,limit: 8)})
      end
    rescue
      e -> %{"beam"=>beam,"stage"=>stage,"oracle_error"=>Exception.message(e)}
    end
    [result|acc]
end
results=Enum.reverse(results)
File.write!(result_path,:json.encode(results))
IO.inspect(Enum.frequencies_by(results,fn row -> {row["stage"],row["equal"],Map.has_key?(row,"oracle_error")} end))
