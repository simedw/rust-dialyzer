Code.require_file("type_codec.exs", __DIR__)
Code.require_file("type_wire.exs", __DIR__)
{:ok, {_, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(:code.which(:dialyzer_succ_typings), [:abstract_code])
forms = Enum.map(forms, fn {:attribute, a, :module, _} -> {:attribute, a, :module, :success_phase_native}; form -> form end)
{:ok, :success_phase_native, bytes} = :compile.forms(forms, [:binary, :export_all])
{:module, :success_phase_native} = :code.load_binary(:success_phase_native, ~c"success_phase_native", bytes)
parse = fn rows -> Enum.map(rows, fn row -> {row["label"], :erl_types.t_fun(Enum.map(row["arguments"], &TypeCodec.type/1), TypeCodec.type(row["result"]))} end) |> Enum.sort() end
encode = fn {label, type} -> %{label: label, arguments: Enum.map(:erl_types.t_fun_args(type), &NativeTypeWire.encode/1), result: NativeTypeWire.encode(:erl_types.t_fun_range(type))} end
outcome = fn action ->
  try do %{status: "ok", value: action.()} catch :error, :function_clause -> %{status: "error", reason: "OTP function_clause"} end
end
for line <- IO.stream(:stdio, :line) do
  row = :json.decode(line)
  old = parse.(row["previous"])
  new = parse.(row["next"])
  IO.puts(:json.encode(%{id: row["id"], strict: outcome.(fn -> :success_phase_native.reached_fixpoint_strict(old,new) end), updated: outcome.(fn -> Enum.map(:success_phase_native.updated_types(old,new),encode) end), persisted: outcome.(fn -> Enum.map(new,fn {key,type}->encode.({key,:erl_types.t_limit(type,5)}) end) end)}))
end
