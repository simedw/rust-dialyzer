[baseline, candidate, prior, output | beams] = System.argv()
Code.compile_string(baseline |> File.read!() |> String.replace("defmodule CoreExport do", "defmodule BaselineCoreExport do"), baseline)
Code.require_file(candidate)
:logger.set_primary_config(:level, :none)
plt = :dialyzer_cplt.from_file(String.to_charlist(prior))
try do
  tables = for i <- 1..5, do: :ets.tab2list(elem(plt, i))
  header = prior |> File.read!() |> :erlang.binary_to_term()
  synthetic = [[], nil, true, false, [1, 2 | :tail], <<5::3>>, <<1, 2>>, -0.0, 0.0,
    1.0, 1, -1, :erlang.bsl(1, 130),
    Map.new(1..32, fn x -> {x, {:nested, [x, x]}} end),
    Map.new(1..33, fn x -> {x, {:nested, [x, x]}} end),
    %{<<1>> => [:binary], :atom => {1, 1.0}, 3 => %{nested: []}}]
  values = [header | tables] ++ synthetic
  Enum.each(values, fn value ->
    old = BaselineCoreExport.capture_terms(value)
    new = CoreExport.capture_terms(value)
    if old !== new, do: raise("complete arena or term IDs differ")
    if :erlang.term_to_binary(CoreExport.rebuild_terms(new |> :json.encode() |> IO.iodata_to_binary() |> :json.decode())) !== :erlang.term_to_binary(value),
      do: raise("native term roundtrip differs")
  end)
  Enum.each(beams, fn path ->
    old = BaselineCoreExport.module(path)
    new = CoreExport.module(path)
    if old !== new, do: raise("complete compiled module differs: #{path}")
  end)
  report = %{complete_arenas_exact: true, native_roundtrips_exact: true,
    term_values: length(values), compiled_modules_exact: length(beams),
    cases: "actual prior header and all five tables; improper lists, bitstrings, signed zero, big integers, maps at32/33 and mixed keys"}
  File.write!(output, :json.encode(report))
  IO.puts(:json.encode(report))
after
  :dialyzer_plt.delete(plt)
end
