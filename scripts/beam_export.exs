defmodule Export do
  def convert(term) when is_tuple(term), do: term |> Tuple.to_list() |> Enum.map(&convert/1)
  def convert(term) when is_list(term), do: Enum.map(term, &convert/1)
  def convert(term) when is_atom(term), do: Atom.to_string(term)
  def convert(term) when is_binary(term) or is_number(term), do: term

  defp runtime_form?({:function, _, name, _, _}) do
    text = Atom.to_string(name)

    name not in [:__info__, :module_info] and not String.starts_with?(text, "MACRO-")
  end

  defp runtime_form?({:attribute, _, :spec, {{name, _}, _}}),
    do: name != :__info__

  defp runtime_form?({:attribute, _, kind, _}), do: kind in [:file, :module]
  defp runtime_form?(_), do: false

  def run(beam_dir, output) do
    modules =
      Path.wildcard(Path.join(beam_dir, "*.beam"))
      |> Enum.map(fn path ->
        {:ok, {name, [abstract_code: {:raw_abstract_v1, forms}]}} =
          :beam_lib.chunks(String.to_charlist(path), [:abstract_code])

        {:attribute, _, :file, {file, _}} =
          Enum.find(forms, &match?({:attribute, _, :file, _}, &1))

        %{
          name: Atom.to_string(name),
          file: List.to_string(file),
          forms: convert(Enum.filter(forms, &runtime_form?/1))
        }
      end)
      |> Enum.sort_by(& &1.name)

    if modules == [], do: raise("no BEAM files found in #{beam_dir}")
    File.mkdir_p!(Path.dirname(output))
    File.write!(output, :json.encode(%{version: 1, modules: modules}))
  end
end

