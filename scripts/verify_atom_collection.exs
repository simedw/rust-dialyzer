[baseline, candidate, prior, output | beams] = System.argv()
paths = [__ENV__.file, baseline, candidate, prior | beams] ++
  Enum.map([baseline, candidate], &Path.join(Path.dirname(&1), "compiler_atom_primitives.exs"))
inventory = fn -> Map.new(paths, fn path ->
  {path, :crypto.hash(:sha256, File.read!(path)) |> Base.encode16(case: :lower)}
end) end
before = inventory.()
Code.compile_string(baseline |> File.read!() |> String.replace("defmodule CoreExport do", "defmodule BaselineCoreExport do"), baseline)
Code.require_file(candidate)
:logger.set_primary_config(:level, :none)
plt = :dialyzer_cplt.from_file(String.to_charlist(prior))
try do
  header = prior |> File.read!() |> :erlang.binary_to_term()
  tables = for i <- 1..5, do: :ets.tab2list(elem(plt, i))
  modules = Enum.map(beams, &CoreExport.module/1)
  synthetic = [nil, true, false, [:same, :same | :tail], <<1, 2>>, <<5::3>>,
    {1, -1, 0.0, :erlang.bsl(1, 130)},
    Map.new(1..33, fn x -> {x, {:nested, [x, :same]}} end),
    %{<<1>> => [:binary], :atom => {1, 1.0}, 3 => %{nested: []}}]
  inputs = [[header], tables, synthetic, [header, tables, synthetic]]
  Enum.each(inputs, fn raw ->
    old = BaselineCoreExport.atom_layout(modules, raw) |> Map.delete(:scope)
    new = CoreExport.atom_layout(modules, raw) |> Map.delete(:scope)
    if old !== new, do: raise("atom names, hashes or layout metadata differ")
  end)
  raw = [header, tables, synthetic]
  expected = BaselineCoreExport.atom_layout(modules, raw) |> Map.delete(:scope)
  samples = for round <- 1..5 do
    order = if rem(round, 2) == 1, do: [BaselineCoreExport, CoreExport], else: [CoreExport, BaselineCoreExport]
    Map.new(order, fn exporter ->
      :erlang.garbage_collect()
      started = System.monotonic_time(:microsecond)
      actual = exporter.atom_layout(modules, raw)
      elapsed = (System.monotonic_time(:microsecond) - started) / 1_000_000
      if Map.delete(actual, :scope) !== expected, do: raise("measured layout differs")
      {if(exporter == CoreExport, do: :candidate, else: :baseline), elapsed}
    end)
  end
  if inventory.() !== before, do: raise("source or inputs changed")
  report = %{atom_layouts_exact: true, inputs_unchanged: true, input_sha256: before, cases: length(inputs),
    local_samples_seconds: samples,
    timing_scope: "five alternating layout pairs in one VM after correctness warmup; collection plus native hash export; GC before each invocation; excludes public CLI startup",
    compiled_modules: length(modules),
    scope: "actual PLT header and all five tables; all supplied modules; synthetic improper lists, maps, bitstrings and repeated atoms; random capture scope excluded"}
  File.write!(output, :json.encode(report))
  IO.puts(:json.encode(Map.delete(report, :input_sha256)))
after
  :dialyzer_plt.delete(plt)
end
