[baseline, candidate, prior, output] = System.argv()
Code.compile_string(baseline |> File.read!() |> String.replace("defmodule CoreExport do", "defmodule BaselineCoreExport do"), baseline)
Code.require_file(candidate)
:logger.set_primary_config(:level, :none)
plt = :dialyzer_cplt.from_file(String.to_charlist(prior))
try do
  values = for i <- 1..5, do: :ets.tab2list(elem(plt, i)) |> Enum.sort()
  values = [prior |> File.read!() |> :erlang.binary_to_term() | values]
  Enum.each(values, fn value ->
    if BaselineCoreExport.capture_terms(value) !== CoreExport.capture_terms(value), do: raise("capture differs")
  end)
  samples = for i <- 1..7 do
    order = if rem(i, 2) == 0, do: [:baseline, :candidate], else: [:candidate, :baseline]
    Map.new(order, fn backend ->
      :erlang.garbage_collect()
      before_reductions = elem(:erlang.process_info(self(), :reductions), 1)
      start = System.monotonic_time(:microsecond)
      Enum.each(values, fn value ->
        case backend do
          :baseline -> BaselineCoreExport.capture_terms(value)
          :candidate -> CoreExport.capture_terms(value)
        end
      end)
      elapsed = (System.monotonic_time(:microsecond) - start) / 1_000_000
      {backend, %{seconds: elapsed, reductions: elem(:erlang.process_info(self(), :reductions), 1) - before_reductions}}
    end)
  end
  tables_before = :ets.all() |> Enum.sort()
  for _ <- 1..20 do
    try do CoreExport.capture_terms(self()) rescue RuntimeError -> :ok end
  end
  if Enum.sort(:ets.all()) !== tables_before, do: raise("ETS tables leak on capture failure")
  File.write!(output, :json.encode(%{samples: samples, actual_arenas_exact: length(values), failure_table_cleanup_exact: true}))
after
  :dialyzer_plt.delete(plt)
end
