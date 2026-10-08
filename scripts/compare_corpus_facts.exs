Code.require_file("type_codec.exs", __DIR__)
Code.require_file("core_export.exs", __DIR__)

defmodule CompareCorpusFacts do
  def location({line, column}), do: {line, column}
  def location(line) when is_integer(line), do: {line, :null}
  def run(manifest, output) do
    cases = manifest |> File.read!() |> :json.decode()
    comparisons = Enum.map(cases, fn c ->
      native = c["native_json"] |> File.read!() |> :json.decode()
      rust = c["rust_json"] |> File.read!() |> :json.decode()
      if native["status"] == "complete" and rust["status"] == "inferred" do
        signatures = Enum.map(native["signatures"], fn s ->
          actual = Enum.find(rust["signatures"], &(&1["named_function"] == Map.take(s, ["module", "name", "arity"])))
          comparison = try do
            if actual do
              result = s["result"]["etf"] |> Base.decode64!() |> :erlang.binary_to_term()
              arguments = Enum.map(s["arguments"], fn t -> t["etf"] |> Base.decode64!() |> :erlang.binary_to_term() end)
              %{arguments_match: arguments == Enum.map(actual["arguments"], &TypeCodec.type/1), result_match: result == TypeCodec.type(actual["result"]), native_result: s["result"]["display"], native_arguments: Enum.map(s["arguments"], & &1["display"]), rust_result: actual["result"], rust_arguments: actual["arguments"]}
            else
              %{arguments_match: false, result_match: false, error: "missing Rust named signature"}
            end
          catch
            kind, reason -> %{arguments_match: false, result_match: false, error: inspect({kind, reason})}
          end
          Map.merge(Map.take(s, ["module", "name", "arity"]), comparison)
        end)
        native_warnings = CoreExport.rebuild_terms(native["warnings"])
        identities = Enum.map(native_warnings, fn {_tag, {_file, loc}, {category, _arguments}} ->
          {line, column} = location(loc)
          {Atom.to_string(category), line, column}
        end) |> Enum.sort()
        actual = Enum.map(rust["warnings"], fn w ->
          {w["details"]["category"], w["line"], if(Enum.all?(identities, fn {_, _, col} -> col == :null end), do: :null, else: w["column"])}
        end) |> Enum.sort()
        %{id: c["id"], signatures: signatures, signature_match: Enum.all?(signatures, &(&1.arguments_match and &1.result_match)),
          warning_category_location_match: identities == actual, native_warning_identities: identities |> Enum.map(&Tuple.to_list/1),
          rust_warning_identities: actual |> Enum.map(&Tuple.to_list/1), details_and_messages_verified: false, option_semantics_verified: false}
      else
        %{id: c["id"], native_status: native["status"], rust_status: rust["status"], details_and_messages_verified: false}
      end
    end)
    File.write!(output, :json.encode(comparisons))
  end
end
[manifest, output] = System.argv()
CompareCorpusFacts.run(manifest, output)
