defmodule CompilerAtomPrimitives do
 def export(atoms,scope) do
  previous=:erts_debug.set_internal_state(:available_internal_state,true)
  try do
   %{version: 1,scope: scope,word_size: :erlang.system_info(:wordsize),atom_hashes: Map.new(Enum.uniq(atoms),fn atom->{Atom.to_string(atom),Integer.to_string(:erts_debug.get_internal_state({:internal_hash,atom}))}end)}
  after
   :erts_debug.set_internal_state(:available_internal_state,previous)
  end
 end
end
