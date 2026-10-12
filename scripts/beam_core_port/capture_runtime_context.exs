[destination] = System.argv()
File.mkdir_p!(destination)
for name <- [:dialyzer_typesig, :dialyzer_contracts] do
  path = :code.which(name)
  {:ok, ^name, chunks} = :beam_lib.all_chunks(path)
  {:ok, {^name, [atoms: atoms]}} = :beam_lib.chunks(path, [:atoms])
  atoms = Map.new(atoms)
  {_, table} = List.keyfind(chunks, ~c"FunT", 0)
  <<count::32, entries::binary>> = table
  rows = for <<name::32, arity::32, label::32, index::32, free::32, uniq::32 <- entries>>,
    do: %{name: Atom.to_string(Map.fetch!(atoms, name)), arity: arity, label: label,
      index: index, env_count: free, uniq: uniq}
  true = length(rows) == count
  profile = %{module: Atom.to_string(name),
    original_sha256: Base.encode16(:crypto.hash(:sha256, File.read!(List.to_string(path)))),
    fun_table: Enum.with_index(rows, fn row, i -> Map.put(row, :new_index, i) end)}
  File.write!(Path.join(destination, "#{name}.json"), :json.encode(profile))
end
