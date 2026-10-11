Code.require_file("compare_terms.exs", __DIR__)
[destination] = System.argv()
rows = :json.decode(File.read!(Path.join(destination,"oracle-results.json")))
diffs = for row <- rows, row["stage"] == "core", row["equal"] == false, reduce: [] do
 acc ->
   index = :json.decode(File.read!(Path.join(destination,"native-results.json"))) |> Enum.find(fn r -> r["beam"]==row["beam"] and r["stage"]=="core" end)
   file=index["native_output"]
   actual=:erlang.binary_to_term(File.read!(file)); expected=:erlang.binary_to_term(File.read!(file<>".oracle.etf"))
   expected_defs=Map.new(elem(expected,5),fn {binder,fun}->{elem(binder,2),fun} end)
   functions=for {binder,fun} <- elem(actual,5), difference=ExactTermComparison.difference(Map.fetch!(expected_defs,elem(binder,2)),fun,"fun"), difference != nil do
    {path,wanted,got}=difference; {name,arity}=elem(binder,2)
    %{module: row["module"],function: Atom.to_string(name),arity: arity,beam: row["beam"],path: path,expected: inspect(wanted,limit: 8),actual: inspect(got,limit: 8)}
   end
   functions++acc
end
File.write!(Path.join(destination,"function-differences.json"),:json.encode(diffs))
IO.puts(:json.encode(%{functions: length(diffs),modules: diffs |> Enum.map(& &1.module) |> Enum.uniq() |> length()}))
