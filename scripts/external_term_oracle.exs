Code.require_file("core_export.exs", __DIR__)
case System.argv() do
  ["generate", path] ->
    for line <- File.stream!(path) do
      row = :json.decode(line)
      term = CoreExport.rebuild_terms(row["arena"])
      for {options, index} <- Enum.with_index([[], [{:compressed, 9}], [{:minor_version, 0}]]) do
        bytes = :erlang.term_to_binary(term, options)
        IO.puts(:json.encode(%{id: "#{row["id"]}/#{index}", bytes: :binary.bin_to_list(bytes)}))
      end
    end
  ["compare", requests, results] ->
    expected = Enum.map(File.stream!(requests), fn line ->
      row = :json.decode(line)
      value = try do {:ok, row["bytes"] |> :binary.list_to_bin() |> :erlang.binary_to_term()} catch _, _ -> :error end
      {row["id"], value}
    end)
    got = Enum.map(File.stream!(results), &:json.decode/1)
    if length(expected) != length(got), do: raise("ETF output denominator changed")
    failures = Enum.zip(expected, got) |> Enum.filter(fn {{id, expected}, got} ->
      if id != got["id"], do: raise("ETF output identity changed")
      case expected do
        :error -> got["status"] != "error"
        {:ok, term} ->
          got["status"] != "ok" or CoreExport.rebuild_terms(got["arena"]) !== term or
            (:erlang.binary_to_term(:binary.list_to_bin(got["encoded"])) !== term)
      end
    end)
    IO.puts(:json.encode(%{commands: length(expected), matched: length(expected) - length(failures), failures: length(failures)}))
    if failures != [], do: raise("ETF parity mismatch: #{inspect(Enum.take(failures, 2), limit: 30)}")
end
