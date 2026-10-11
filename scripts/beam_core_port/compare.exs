Code.require_file("compare_terms.exs", __DIR__)
# Oracle-only comparison: the native production command never invokes this VM.
[expected_path, actual_path] = System.argv()
expected = expected_path |> File.read!() |> :erlang.binary_to_term()
actual = actual_path |> File.read!() |> :erlang.binary_to_term()
case ExactTermComparison.difference(expected,actual,"root") do
  nil -> IO.puts("exact term equality")
  {path, a,b} -> IO.puts("mismatch #{path}\nexpected #{inspect(a, limit: 12)}\nactual   #{inspect(b, limit: 12)}"); System.halt(1)
end
