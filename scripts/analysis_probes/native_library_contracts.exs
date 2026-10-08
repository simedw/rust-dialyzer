[root,core_path,dependency_path,out]=System.argv()
Code.require_file(Path.join(root,"scripts/native_types.exs"))
core=:json.decode(File.read!(core_path)); dependencies=:json.decode(File.read!(dependency_path))
targets=MapSet.new(core["modules"],& &1["name"])
plt=:dialyzer_cplt.from_file(String.to_charlist(dependencies["source"]["path"]))
try do
 rows=for {kind,contracts}<-[{"contract",:dialyzer_plt.get_all_contracts(plt)},{"callback",:dialyzer_plt.get_all_callbacks(plt)}],{{m,f,a},raw}<-contracts,not MapSet.member?(targets,Atom.to_string(m)) do
  raw=case raw do {_,{:contract,_,_,_}=contract,_}->contract;{:contract,_,_,_}=contract->contract end
  %{kind: kind,mfa: %{module: Atom.to_string(m),function: Atom.to_string(f),arity: a},record: NativeTypes.term(raw)}
 end
 File.write!(out,Enum.map_join(rows,"",&(IO.iodata_to_binary(:json.encode(&1))<>"\n")))
 IO.inspect(%{records: length(rows)})
after
 :dialyzer_plt.delete(plt)
end
