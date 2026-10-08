defmodule NativeTypes do
  def term(value) when is_atom(value), do: %{kind: "atom", value: Atom.to_string(value)}
  def term(value) when is_integer(value), do: %{kind: "integer", value: Integer.to_string(value)}
  def term(value) when is_float(value), do: %{kind: "float", value: Base.encode16(<<value::float-64>>)}
  def term(value) when is_binary(value), do: %{kind: "binary", value: Base.encode64(value)}
  def term([]), do: %{kind: "nil"}
  def term([_ | _] = value) do
    {items, tail} = parts(value, [])
    %{kind: "list", items: Enum.map(items, &term/1), tail: term(tail)}
  end
  def term(value) when is_tuple(value), do: %{kind: "tuple", items: value |> Tuple.to_list() |> Enum.map(&term/1)}
  def term(value) when is_map(value), do: %{kind: "map", entries: value |> :maps.to_list() |> Enum.sort() |> Enum.map(fn {k, v} -> [term(k), term(v)] end)}
  defp parts([h | t], acc), do: parts(t, [h | acc])
  defp parts(tail, acc), do: {Enum.reverse(acc), tail}

  def type(value) do
    true = :erl_types.is_erl_type(value)
    display = try do
      %{display: value |> :erl_types.t_to_string() |> List.to_string()}
    catch
      kind, reason -> %{display: :null, display_error: %{kind: kind, reason: term(reason)}}
    end
    Map.merge(%{native: term(value), etf: Base.encode64(:erlang.term_to_binary(value))}, display)
  end

  def domains(:unknown), do: %{status: "unknown"}
  def domains(types), do: %{status: "known", arguments: Enum.map(types, &type/1)}

  def decode(%{"atom" => name}), do: String.to_atom(name)
  def decode(%{"integer" => value}), do: if(is_integer(value), do: value, else: String.to_integer(value))
  def decode(%{"list" => values}), do: Enum.map(values, &decode/1)
  def decode(%{"tuple" => values}), do: values |> Enum.map(&decode/1) |> List.to_tuple()
  def decode(%{"type_etf" => value}) do
    type = value |> Base.decode64!() |> :erlang.binary_to_term([:safe])
    true = :erl_types.is_erl_type(type)
    type
  end
  def decode(%{"constructor" => name} = value) do
    arguments = Enum.map(Map.get(value, "arguments", []), &decode/1)
    constructor = String.to_existing_atom(name)
    if not String.starts_with?(name, "t_") or not function_exported?(:erl_types, constructor, length(arguments)),
      do: raise("unknown erl_types constructor #{name}/#{length(arguments)}")
    type = apply(:erl_types, constructor, arguments)
    true = :erl_types.is_erl_type(type)
    type
  end

  def request(%{"mfa" => mfa} = request) do
    module = String.to_atom(mfa["module"])
    function = String.to_atom(mfa["function"])
    arity = mfa["arity"]
    true = is_integer(arity) and arity >= 0 and arity <= 255
    domains = :erl_bif_types.arg_types(module, function, arity)
    base = %{mfa: mfa, is_known: :erl_bif_types.is_known(module, function, arity), domains: domains(domains)}
    case request["operation"] do
      "domains" -> Map.put(base, :return_on_any, type(:erl_bif_types.type(module, function, arity)))
      "transfer" ->
        arguments = Enum.map(request["arguments"], &decode/1)
        true = length(arguments) == arity
        true = Enum.all?(arguments, &:erl_types.is_erl_type/1)
        admissible = if domains == :unknown, do: nil, else: :erl_types.t_inf_lists(arguments, domains)
        Map.merge(base, %{arguments: Enum.map(arguments, &type/1),
          admissible_arguments: if(admissible, do: Enum.map(admissible, &type/1), else: nil),
          return: type(:erl_bif_types.type(module, function, arity, arguments))})
      other -> raise("unknown operation #{inspect(other)}")
    end
  end
end
