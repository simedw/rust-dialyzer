for line <- IO.stream(:stdio, :line) do
  request = :json.decode(line)
  initial = Enum.map(request["initial"], &String.to_atom/1) |> Enum.sort()
  options = Enum.map(request["options"], &String.to_atom/1)
  response = try do
    %{tags: :dialyzer_options.build_warnings(options, initial) |> Enum.map(&Atom.to_string/1)}
  catch
    :throw, {:dialyzer_options_error, _} -> %{error: "unknown_option"}
  end
  IO.puts(:json.encode(response))
end
