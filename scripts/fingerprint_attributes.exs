Code.require_file("core_export.exs", __DIR__)
[directory] = System.argv()
for size <- [0, 1, 31, 32, 33, 64, 80, 255], keys <- [:integer, :tuple, :mixed, :atom] do
  name = String.to_atom("fingerprint_#{size}_#{keys}")
  atom_keys = if keys == :atom and size > 0 do
    Map.new(size..1//-1, fn i -> {i, String.to_atom("#{name}_key_#{i}")} end)
  else
    %{}
  end
  pairs = for i <- if(size == 0, do: [], else: 1..size) do
    key = case keys do
      :atom -> Map.fetch!(atom_keys, i)
      :integer -> i
      :tuple -> {:key, i}
      :mixed -> if rem(i, 2) == 0, do: i / 2, else: div(i, 2)
    end
    {key, {:value, i}}
  end
  value = Map.new(pairs)
  forms = [{:attribute, 1, :file, {~c"/original/path/maps.erl", 1}},
    {:attribute, 1, :module, name}, {:attribute, 1, :export, [{:f, 0}]},
    {:attribute, 1, :earlier_atom, Map.get(atom_keys, 1, :none)},
    {:attribute, 1, :map_attribute, {:nested, [value]}},
    {:function, 1, :f, 0, [{:clause, 1, [], [], [{:atom, 1, :ok}]}]}, {:eof, 2}]
  {:ok, ^name, beam} = :compile.forms(forms, [:binary, :debug_info])
  folder = Path.join(directory, Atom.to_string(name))
  File.mkdir_p!(folder)
  File.write!(Path.join(folder, "#{name}.beam"), beam)
  core = Path.join(folder, "core.json")
  CoreExport.run(folder, core)
  IO.puts(:json.encode(%{id: Atom.to_string(name), core_json: core, beams_dir: folder}))
end
