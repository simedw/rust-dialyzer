# Oracle-only benchmark of current bridge stages; input files read before timing.
# Drop each module result before compiling the next, matching the native loop.
[stage, count, manifest] = System.argv()
count = String.to_integer(count)
opts = [:no_copt, :to_core, :binary, :return_errors, :no_inline, :strict_record_tests, :dialyzer, :no_spawn_compiler_process]
inputs = manifest |> File.read!() |> String.split("\n",trim: true) |> Enum.map(&File.read!/1)
compile = fn bytes ->
  {:ok,{module,[debug_info: {:debug_info_v1,backend,data}]}} = :beam_lib.chunks(bytes,[:debug_info])
  {:ok,forms} = backend.debug_info(:erlang_v1,module,data,opts)
  if stage == "forms" do
    forms
  else
    {:ok,core} = backend.debug_info(:core_v1,module,data,opts)
    {forms,:dialyzer_clean_core.clean(core)}
  end
end
Enum.each(inputs,compile)
batches = for _ <- 1..5 do
  {microseconds,_} = :timer.tc(fn -> Enum.each(1..count, fn _ -> Enum.each(inputs, compile) end) end)
  microseconds/count/1000
end
sorted=Enum.sort(batches)
IO.puts(:json.encode(%{"stage"=>stage,"manifest"=>manifest,"modules"=>length(inputs),"median_ms"=>Enum.at(sorted,2),"batch_ms"=>batches,"iterations_per_batch"=>count,"scope"=>"BEAM decode + forms, optionally raw Core + clean; filesystem IO and ETF transport excluded"}))
