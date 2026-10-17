defmodule NativeTypeWire do
  def scalar(kind), do: %{"kind" => kind}
  def encode(:any), do: scalar("any")
  def encode(:none), do: scalar("none")
  def encode(:unit), do: scalar("unit")
  def encode(:opaque), do: scalar("opaque")
  def encode({:c, :atom, :any, _}), do: scalar("atom")

  def encode({:c, :atom, values, _}),
    do: %{"kind" => "atoms", "values" => Enum.map(values, &Atom.to_string/1)}

  def encode({:c, :binary, {unit, base}, _}),
    do: %{"kind" => "bitstring", "unit" => to_string(unit), "base" => to_string(base)}

  def encode({:c, :function, {domain, range}, _}) do
    args =
      case domain do
        :any -> :null
        {:c, :product, types, _} -> Enum.map(types, &encode/1)
      end

    %{"kind" => "function", "domain" => args, "range" => encode(range)}
  end

  def encode({:c, :identifier, :any, _}), do: scalar("identifier")

  def encode({:c, :identifier, ids, _}),
    do: %{"kind" => "identifiers", "values" => Enum.map(ids, &Atom.to_string/1)}

  def encode({:c, :list, {elements, tail}, size}),
    do: %{
      "kind" => "list",
      "elements" => encode(elements),
      "tail" => encode(tail),
      "nonempty" => size == :nonempty
    }

  def encode({:c, nil, _, _}), do: scalar("nil")
  def encode({:c, :number, :any, :unknown}), do: scalar("number")
  def encode({:c, :number, :any, :float}), do: scalar("float")
  def encode({:c, :number, :any, :integer}), do: scalar("integer")

  def encode({:c, :number, {:int_set, values}, :integer}),
    do: %{"kind" => "integers", "values" => Enum.map(values, &to_string/1)}

  def encode({:c, :number, {:int_rng, from, to}, :integer}),
    do: %{"kind" => "range", "from" => to_string(from), "to" => to_string(to)}

  def encode({:c, :tuple, :any, {:any, :any}}), do: scalar("tuple_any")

  def encode({:c, :tuple, elements, {_, _}}),
    do: %{"kind" => "tuple", "elements" => Enum.map(elements, &encode/1)}

  def encode({:c, :tuple_set, groups, _}),
    do: %{
      "kind" => "tuple_set",
      "values" => Enum.flat_map(groups, fn {_, ts} -> Enum.map(ts, &encode/1) end)
    }

  def encode({:c, :record, {identity, fields}, _}) do
    name = case identity do
      nil -> :null
      {module, name} -> %{"module" => Atom.to_string(module), "name" => Atom.to_string(name)}
      name when is_atom(name) -> Atom.to_string(name)
    end
    %{"kind" => "record", "name" => name, "fields" => fields |> Enum.sort() |> Enum.map(fn
      {key, :missing} -> %{"key" => Atom.to_string(key), "presence" => "missing"}
      {key, {:present, value}} -> %{"key" => Atom.to_string(key), "presence" => "present", "value" => encode(value)}
    end)}
  end
  def encode({:c, :record_set, records, _}), do: %{"kind" => "record_set", "values" => Enum.map(records, &encode/1)}

  def encode({:c, :map, {pairs, dk, dv}, _}),
    do: %{
      "kind" => "map",
      "entries" =>
        Enum.map(pairs, fn {k, p, v} ->
          %{"key" => encode(k), "presence" => to_string(p), "value" => encode(v)}
        end),
      "default_key" => encode(dk),
      "default_value" => encode(dv)
    }

  def encode({:c, :product, types, _}),
    do: %{"kind" => "product", "values" => Enum.map(types, &encode/1)}

  def encode({:c, :var, id, _}),
    do: %{
      "kind" => "var",
      "id" => if(is_atom(id), do: %{"atom" => to_string(id)}, else: %{"integer" => to_string(id)})
    }

  def encode({:c, :union, types, _}),
    do: %{
      "kind" => "union",
      "values" => types |> Enum.reject(&(&1 == :none)) |> Enum.map(&encode/1)
    }

  def encode({:c, :nominal, {name, inner}, _}),
    do: %{
      "kind" => if(inner in [:none, :unit], do: "nominal_raw", else: "nominal"),
      "name" => name(name),
      "inner" => encode(inner)
    }

  def encode({:c, :nominal_set, {ns, s}, _}),
    do: %{
      "kind" => "nominal_set",
      "nominals" => Enum.map(ns, &encode/1),
      "structural" => encode(s)
    }

  def name(name) when is_atom(name), do: %{"symbol" => to_string(name)}

  def name({module, name, arity, opacity}),
    do: %{
      "module" => to_string(module),
      "name" => to_string(name),
      "arity" => arity,
      "opacity" => to_string(opacity)
    }
end
