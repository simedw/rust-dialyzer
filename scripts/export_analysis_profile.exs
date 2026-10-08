{module,out}=case System.argv() do [out]->{:dialyzer_typesig,out};[module,out]->{String.to_atom(module),out}end
path=:code.which(module)
{:ok,^module,chunks}=:beam_lib.all_chunks(path)
{:ok,{^module,[{:atoms,atoms}]}}=:beam_lib.chunks(path,[:atoms])
atoms=Map.new(atoms)
{_,table}=List.keyfind(chunks,~c"FunT",0)
<<count::32,entries::binary>>=table
fun_table=for <<name::32,arity::32,label::32,index::32,free::32,uniq::32<-entries>>,do: %{name: Atom.to_string(Map.fetch!(atoms,name)),arity: arity,label: label,index: index,env_count: free,uniq: uniq}
true=length(fun_table)==count
fun_table=Enum.with_index(fun_table,fn row,new_index->Map.put(row,:new_index,new_index)end)
profile=%{module: Atom.to_string(module),original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))),fun_table: fun_table}
File.write!(out,[:json.encode(profile),"\n"])
IO.inspect(%{functions: count,beam_sha256: profile.original_sha256})
