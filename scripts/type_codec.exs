defmodule TypeCodec do
  def integer("neg_inf"), do: :neg_inf
  def integer("pos_inf"), do: :pos_inf
  def integer(value) when is_binary(value), do: String.to_integer(value)
  def integer(value) when is_integer(value), do: value
  def type(%{"kind" => kind} = value) do
    case kind do
      "record" -> {:c, :record, {record_name(value["name"]), Map.new(value["fields"] || [], fn field ->
        {String.to_atom(field["key"]), if(field["presence"] == "missing", do: :missing, else: {:present, type(field["value"])})}
      end)}, :unknown}
      "record_set" -> {:c, :record_set, Enum.map(value["values"], &type/1), :unknown}
      "unification_nominal_set" -> {:c,:nominal_set,{type(value["nominal"]),type(value["structural"])},:unknown}
      "union_raw" -> {:c,:union,Enum.map(value["values"],&type/1),:unknown}
      "product_raw" -> {:c,:product,Enum.map(value["values"],&type/1),:unknown}
      "nominal_raw" -> {:c,:nominal,{nominal_name(value["name"]),type(value["inner"])},:unknown}
      "nominal" -> :erl_types.t_nominal(nominal_name(value["name"]),type(value["inner"]))
      "nominal_set" -> {:c,:nominal_set,{Enum.map(value["nominals"],&type/1),type(value["structural"])},:unknown}
      "map_any" -> :erl_types.t_map()
      "map" -> :erl_types.t_map(Enum.map(value["entries"], fn e->{type(e["key"]),String.to_atom(e["presence"]),type(e["value"])} end),type(value["default_key"]),type(value["default_value"]))
      "any" -> :erl_types.t_any()
      "none" -> :erl_types.t_none()
      "unit" -> :erl_types.t_unit()
      "atom" -> :erl_types.t_atom()
      "atoms" -> value["values"] |> Enum.map(&(:erl_types.t_atom(String.to_atom(&1)))) |> :erl_types.t_sup()
      "integer" -> :erl_types.t_integer()
      "integers" -> value["values"] |> Enum.map(&integer/1) |> :erl_types.t_integers()
      "range" -> :erl_types.t_from_range(integer(value["from"]), integer(value["to"]))
      "float" -> :erl_types.t_float()
      "number" -> :erl_types.t_number()
      "bitstring" -> :erl_types.t_bitstr(integer(value["unit"]), integer(value["base"]))
      "binary" -> :erl_types.t_binary()
      "function" -> if value["domain"] in [nil, :null], do: :erl_types.t_fun(type(value["range"])), else: :erl_types.t_fun(Enum.map(value["domain"], &type/1), type(value["range"]))
      "product" -> value["values"] |> Enum.map(&type/1) |> :erl_types.t_product()
      "var" -> :erl_types.t_var(if value["id"]["atom"], do: String.to_atom(value["id"]["atom"]), else: integer(value["id"]["integer"]))
      "opaque" -> :opaque
      "identifier" -> :erl_types.t_identifier()
      "identifiers" -> value["values"] |> Enum.map(fn name -> case name do
        "pid" -> :erl_types.t_pid()
        "port" -> :erl_types.t_port()
        "reference" -> :erl_types.t_reference()
      end end) |> :erl_types.t_sup()
      "nil" -> :erl_types.t_nil()
      "list" ->
        elements = type(value["elements"])
        tail = type(value["tail"])
        if elements in [:none, :unit] or tail in [:none, :unit] do
          :none
        else
          {:c, :list, {elements, tail}, if(value["nonempty"], do: :nonempty, else: :unknown)}
        end
      "proper_list" -> if value["nonempty"], do: :erl_types.t_nonempty_list(type(value["elements"])), else: :erl_types.t_list(type(value["elements"]))
      "cons" -> :erl_types.t_cons(type(value["head"]), type(value["tail"]))
      "tuple_any" -> :erl_types.t_tuple()
      "tuple_arity" -> :erl_types.t_tuple(value["arity"])
      "tuple" -> value["elements"] |> Enum.map(&type/1) |> :erl_types.t_tuple()
      "tuple_set" -> value["values"] |> Enum.map(&type/1) |> :erl_types.t_sup()
      "union" -> value["values"] |> Enum.map(&type/1) |> :erl_types.t_sup()
      other -> raise("unsupported type family #{other}")
    end
  end
  def record_name(value) when value in [nil, :null], do: nil
  def record_name(value) when is_binary(value), do: String.to_atom(value)
  def record_name(%{"module" => module, "name" => name}), do: {String.to_atom(module), String.to_atom(name)}
  def nominal_name(%{"symbol"=>name}), do: String.to_atom(name)
  def nominal_name(value), do: {String.to_atom(value["module"]),String.to_atom(value["name"]),value["arity"],String.to_atom(value["opacity"])}
end
