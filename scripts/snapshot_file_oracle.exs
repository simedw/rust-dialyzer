Code.require_file("core_export.exs", __DIR__)
defmodule SnapshotFileOracle do
  def load(module, alias_name) do
    {:ok, {^module, [{:abstract_code, {:raw_abstract_v1, forms}}]}} =
      :beam_lib.chunks(:code.which(module), [:abstract_code])
    forms = Enum.map(forms, fn
      {:attribute, line, :module, ^module} -> {:attribute, line, :module, alias_name}
      other -> other
    end)
    {:ok, ^alias_name, bytes} = :compile.forms(forms, [:binary, :export_all])
    {:module, ^alias_name} = :code.load_binary(alias_name, ~c"snapshot_file_oracle", bytes)
  end
  def module("classic"), do: :snapshot_cplt_oracle
  def module("incremental"), do: :snapshot_iplt_oracle
  def read(request), do: apply(module(request["kind"]), :get_record_from_file, [String.to_charlist(request["path"])])
  def decoded(value) when is_binary(value), do: :erlang.binary_to_term(value)
  def decoded(value), do: value
  def compare(request, result) do
    if request["id"] != result["id"], do: raise("snapshot identity changed")
    case read(request) do
      {:error, error} ->
        expected = %{no_such_file: "NoSuchFile", read_error: "ReadError", not_valid: "NotValid"}[error]
        result["error"] == expected
      {:ok, record} ->
        correct = is_nil(result["error"]) and (request["omit_arena"] == true or CoreExport.rebuild_terms(result["arena"]) === record)
        correct = if request["incremental_data"] do
          native = try do {:ok, :snapshot_iplt_oracle.get_incremental_data(record)} catch _, _ -> :error end
          correct and case native do
            {:ok, value} -> CoreExport.rebuild_terms(result["incremental_data"]) === value
            :error -> not is_nil(result["incremental_data"]["error"])
          end
        else
          correct
        end
        correct = if request["decode_info"] do
          native = try do {:ok, :erlang.binary_to_term(elem(record, 3))} catch _, _ -> :error end
          correct and case native do
            {:ok, value} -> CoreExport.rebuild_terms(result["info"]) === value
            :error -> not is_nil(result["info"]["error"])
          end
        else
          correct
        end
        if request["write_path"] do
          output = Map.put(request, "path", request["write_path"])
          correct and is_nil(result["write_error"]) and read(output) === {:ok, record}
        else
          correct
        end
    end
  end
  def tables(plt), do: (for i <- 1..5, do: plt |> elem(i) |> :ets.tab2list() |> Enum.sort())
  def read_plt(kind, path), do: apply(if(kind == "classic", do: :dialyzer_cplt, else: :dialyzer_iplt), :from_file, [String.to_charlist(path)])
end
SnapshotFileOracle.load(:dialyzer_cplt, :snapshot_cplt_oracle)
SnapshotFileOracle.load(:dialyzer_iplt, :snapshot_iplt_oracle)
case System.argv() do
  ["generate", directory, base] ->
    File.mkdir_p!(directory)
    binary = &:erlang.term_to_binary(&1, [{:compressed, 9}])
    for i <- 0..199, kind <- ["classic", "incremental"], legacy <- [false, true] do
      tag = if kind == "classic", do: :file_plt, else: :ifile_plt
      data = {:incremental_data, :dict.from_list([{String.to_atom("m#{i}"), [:a, :b]}]), %{a: [{:warn_return_no_exit, {~c"a.erl", i, {:a, :f, 0}}, {:no_return, [:only_normal, :f, 0]}}]}, [:warn_return_no_exit]}
      info = %{{:a, :f, 0} => {:erl_types.t_integer(i), []}}
      record = {tag, ~c"5.4", [{:a, <<i::128>>}], binary.(info), binary.(%{}), binary.(%{}), binary.([]), binary.(%{}), if(legacy, do: data, else: binary.(data)), []}
      path = Path.join(directory, "#{i}-#{kind}-#{legacy}.plt")
      File.write!(path, :erlang.term_to_binary(record, if(legacy, do: [{:compressed, 9}], else: [])))
      IO.puts(:json.encode(%{id: "#{i}/#{kind}/#{legacy}", kind: kind, path: path, incremental_data: kind == "incremental", decode_info: true, write_path: path <> ".rust"}))
    end
    for kind <- ["classic", "incremental"], {term, i} <- Enum.with_index([:bad, {:file_plt}, List.to_tuple([if(kind == "classic", do: :file_plt, else: :ifile_plt) | Enum.to_list(1..9)]), List.to_tuple([:other | Enum.to_list(1..9)])]) do
      path = Path.join(directory, "shape-#{kind}-#{i}.plt")
      File.write!(path, binary.(term))
      IO.puts(:json.encode(%{id: "shape/#{kind}/#{i}", kind: kind, path: path}))
    end
    for kind <- ["classic", "incremental"], {bytes, i} <- Enum.with_index([<<>>, <<131>>, <<131, 255>>, <<0>>, <<131, 80, 0, 0, 0, 1, 0>>]) do
      path = Path.join(directory, "bad-#{kind}-#{i}.plt")
      File.write!(path, bytes)
      IO.puts(:json.encode(%{id: "bad/#{kind}/#{i}", kind: kind, path: path}))
    end
    for kind <- ["classic", "incremental"], {path, suffix} <- [{Path.join(directory, "missing"), "missing"}, {directory, "directory"}] do
      IO.puts(:json.encode(%{id: "io/#{kind}/#{suffix}", kind: kind, path: path}))
    end
    plt = :dialyzer_cplt.from_file(String.to_charlist(base))
    path = Path.join(directory, "native-to-file.iplt")
    :ok = :dialyzer_iplt.to_file_custom_vsn(String.to_charlist(path), plt, :dict.new(), {:iplt_info, [], :dict.new(), :none, :none}, :none, [])
    :dialyzer_plt.delete(plt)
    for {kind, path, id} <- [{"classic", base, "base-classic"}, {"incremental", path, "native-incremental"}] do
      IO.puts(:json.encode(%{id: id, kind: kind, path: path, write_path: Path.join(directory, id <> ".rust"), incremental_data: false, decode_info: false, load_tables: true, omit_arena: true, max_nodes: 8000000, max_expanded: 8000000}))
    end
  ["compare", requests, results] ->
    requests = Enum.map(File.stream!(requests), &:json.decode/1)
    results = Enum.map(File.stream!(results), &:json.decode/1)
    if length(requests) != length(results), do: raise("snapshot denominator changed")
    failures = Enum.zip(requests, results) |> Enum.filter(fn {r, s} -> not SnapshotFileOracle.compare(r, s) end)
    for request <- requests, request["load_tables"] do
      before = SnapshotFileOracle.read_plt(request["kind"], request["path"])
      after_plt = SnapshotFileOracle.read_plt(request["kind"], request["write_path"])
      if SnapshotFileOracle.tables(before) !== SnapshotFileOracle.tables(after_plt), do: raise("native PLT tables changed")
      :dialyzer_plt.delete(before)
      :dialyzer_plt.delete(after_plt)
    end
    IO.puts(:json.encode(%{commands: length(requests), matched: length(requests) - length(failures), failures: Enum.map(failures, fn {r, s} -> %{id: r["id"], error: s["error"]} end), native_table_reloads: 2}))
    if failures != [], do: raise("snapshot parity failed")
end
