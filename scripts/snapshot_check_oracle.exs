defmodule SnapshotCheckBoundary do
  def record(_), do: Process.get(:snapshot)
  def hash(file) do
    module = file |> List.to_string() |> Path.basename(".beam")
    Map.fetch!(:persistent_term.get({__MODULE__, :hashes}), module)
  end
end

defmodule SnapshotCheckOracle do
  def fields(forms, record) do
    {:attribute, _, :record, {^record, fields}} = Enum.find(forms, &match?({:attribute, _, :record, {^record, _}}, &1))
    Enum.map(fields, fn field ->
      field = if elem(field, 0) == :typed_record_field, do: elem(field, 1), else: field
      elem(elem(field, 2), 2)
    end)
  end
  def put(record, fields, key, value), do: put_elem(record, Enum.find_index(fields, &(&1 == key)) + 1, value)
  def version({:record_field, _, {:atom, _, :version}, value}), do: value
  def version(tuple) when is_tuple(tuple), do: version(Tuple.to_list(tuple))
  def version(list) when is_list(list), do: Enum.find_value(list, &version/1)
  def version(_), do: nil
  def load do
    {:ok, {_, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(:code.which(:dialyzer_iplt), [:abstract_code])
    version_form = Enum.find(forms, &match?({:function, _, :check_version, _, _}, &1)) |> version()
    {:value, version, _} = :erl_eval.expr(version_form, :erl_eval.new_bindings())
    helpers = ~c"record() -> #ifile_plt{}. data() -> #incremental_data{}. options() -> #options{}. get_record_from_file(File) -> 'Elixir.SnapshotCheckBoundary':record(File). compute_md5_from_file(File) -> 'Elixir.SnapshotCheckBoundary':hash(File)."
    {:ok, tokens, _} = :erl_scan.string(helpers)
    {parsed, tail} = Enum.reduce(tokens, {[], []}, fn token, {parsed, tail} ->
      tail = tail ++ [token]
      if elem(token, 0) == :dot do
        {:ok, form} = :erl_parse.parse_form(tail)
        {parsed ++ [form], []}
      else
        {parsed, tail}
      end
    end)
    [] = tail
    copy = forms |> Enum.reject(&match?({:function, _, name, _, _} when name in [:get_record_from_file, :compute_md5_from_file], &1)) |> Enum.reject(&match?({:eof, _}, &1)) |> Enum.map(fn {:attribute, a, :module, _} -> {:attribute, a, :module, :rust_snapshot_check_native}; form -> form end)
    {:ok, :rust_snapshot_check_native, binary} = :compile.forms(copy ++ parsed, [:binary, :export_all])
    {:module, :rust_snapshot_check_native} = :code.load_binary(:rust_snapshot_check_native, ~c"snapshot_check", binary)
    {fields(forms, :ifile_plt), fields(forms, :incremental_data), fields(forms, :options), version}
  end
  def hashes(map), do: Map.new(map, fn {module, digest} -> {String.to_atom(module), digest} end)
  def warning_tags(:null), do: :none
  def warning_tags(tags), do: Enum.map(tags, &String.to_atom/1)
end
{record_fields, data_fields, option_fields, native_version} = SnapshotCheckOracle.load()
for line <- IO.stream(:stdio, :line) do
  r = :json.decode(line)
  old = r["previous"]
  :persistent_term.put({SnapshotCheckBoundary, :hashes}, Map.merge(r["current"], r["profile"]["implementation"]))
  if old == :null do
    Process.put(:snapshot, {:error, :no_such_file})
  else
    data = :rust_snapshot_check_native.data()
    data = SnapshotCheckOracle.put(data, data_fields, :mod_deps, :dict.new())
    data = SnapshotCheckOracle.put(data, data_fields, :warning_map, if(old["warnings_available"], do: %{}, else: :none))
    data = SnapshotCheckOracle.put(data, data_fields, :legal_warnings, SnapshotCheckOracle.warning_tags(old["legal_warnings"]))
    record = :rust_snapshot_check_native.record()
    record = SnapshotCheckOracle.put(record, record_fields, :version, if(old["profile"]["analysis_version"] == r["profile"]["analysis_version"], do: native_version, else: ~c"old"))
    record = SnapshotCheckOracle.put(record, record_fields, :module_md5_list, SnapshotCheckOracle.hashes(old["modules"]) |> Enum.sort())
    record = SnapshotCheckOracle.put(record, record_fields, :implementation_md5, SnapshotCheckOracle.hashes(old["profile"]["implementation"]) |> Enum.sort())
    record = SnapshotCheckOracle.put(record, record_fields, :incremental_data, :erlang.term_to_binary(data))
    Process.put(:snapshot, {:ok, record})
  end
  options = SnapshotCheckOracle.put(:rust_snapshot_check_native.options(), option_fields, :legal_warnings, SnapshotCheckOracle.warning_tags(r["legal_warnings"]))
  files = Map.keys(r["current"]) |> Enum.map(&String.to_charlist("/probe/" <> &1 <> ".beam"))
  result = :rust_snapshot_check_native.check_incremental_plt1(~c"snapshot", options, files)
  check = case result do
    {:ok, info, _} -> if elem(info, 3) == :none, do: "missing_warnings", else: "fresh"
    {:differ, _, _, _, :none, _} -> "changed_without_warnings"
    {:differ, _, _, _, _, _} -> "changed"
    {:legal_warnings_changed, _, _} -> "warning_profile_changed"
    {:old_version, _} -> "stale_version"
    {:old_version, _, _} -> "stale_version"
    {:new_file, _, _} -> "missing_file"
  end
  IO.puts(:json.encode(%{id: r["id"], check: check}))
end
