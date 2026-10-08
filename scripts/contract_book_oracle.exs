Code.require_file("type_codec.exs", __DIR__)
{:ok, {:dialyzer_succ_typings, [{:abstract_code, {:raw_abstract_v1, forms}}]}} = :beam_lib.chunks(:code.which(:dialyzer_succ_typings), [:abstract_code])
defmodule NewContractExpression do
  def find({:match, _, {:var, _, :NewPltContracts}, expression}), do: [expression]
  def find(tuple) when is_tuple(tuple), do: tuple |> Tuple.to_list() |> Enum.flat_map(&find/1)
  def find(list) when is_list(list), do: Enum.flat_map(list, &find/1)
  def find(_), do: []
  def decode(%{"kind" => "atom", "value" => v}), do: String.to_atom(v)
  def decode(%{"kind" => "integer", "value" => v}), do: String.to_integer(v)
  def decode(%{"kind" => "tuple", "items" => xs}), do: xs |> Enum.map(&decode/1) |> List.to_tuple()
  def decode(%{"kind" => "nil"}), do: []
  def decode(%{"kind" => "list", "items" => xs, "tail" => tail}), do: Enum.reduce(Enum.reverse(xs), decode(tail), fn x, t -> [decode(x) | t] end)
end
[target] = Enum.filter(forms, fn x -> match?({:function, _, :find_succ_types_for_scc, _, _}, x) end)
[expression] = NewContractExpression.find(target)
function = {:function, 0, :filter_new, 2, [{:clause, 0, [{:var, 0, :PltContracts}, {:var, 0, :Plt}], [], [expression]}]}
{:ok, :new_contract_oracle, binary} = :compile.forms([{:attribute, 0, :module, :new_contract_oracle}, function], [:binary, :export_all])
{:module, :new_contract_oracle} = :code.load_binary(:new_contract_oracle, ~c"new_contract_oracle", binary)
[source, requests] = System.argv()
contracts = source |> File.read!() |> :json.decode() |> Map.fetch!("contracts")
graph = :dialyzer_callgraph.new()
for line <- File.stream!(requests) do
  input = :json.decode(line)
  entry = Enum.at(contracts, input["contract_index"])
  mfa = {String.to_atom(entry["mfa"]["module"]), String.to_atom(entry["mfa"]["function"]), entry["mfa"]["arity"]}
  :ets.insert(elem(graph, 5), {0, mfa})
  candidate = entry["contract"]
  alternatives = Enum.map(candidate["alternatives"], fn a -> {NewContractExpression.decode(a["signature"]), Enum.map(a["constraints"], fn c -> {:subtype, NewContractExpression.decode(c["left"]), NewContractExpression.decode(c["right"])} end)} end)
  contract = {:contract, alternatives, Enum.map(candidate["arguments"], &NewContractExpression.decode/1), []}
  plt = :dialyzer_plt.new()
  added = Enum.map(input["successes"], fn success ->
    accepted = :dialyzer_contracts.check_contracts([{mfa, contract}], graph, [{0, TypeCodec.type(success)}])
    new = :new_contract_oracle.filter_new(accepted, plt)
    :dialyzer_plt.insert_contract_list(plt, new)
    new != []
  end)
  output = %{added: added, active: :dialyzer_plt.is_contract(plt, mfa)}
  :dialyzer_plt.delete(plt)
  IO.puts(:json.encode(output))
end
:dialyzer_callgraph.delete(graph)
