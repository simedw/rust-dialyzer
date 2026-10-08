Code.require_file("native_types.exs",__DIR__)
Code.require_file("../core_export.exs",__DIR__)
defmodule FormsOracle do
 def decode(module) do
  Enum.reduce(module["terms"],:array.new(),fn term,array ->
   get=fn id -> :array.get(id,array) end
   value=case term["kind"] do
    "atom" -> String.to_atom(term["value"])
    "integer" -> String.to_integer(term["value"])
    "float" -> <<v::float-64>> = Base.decode16!(term["value"]);v
    "binary" -> Base.decode64!(term["value"])
    "bitstring" -> bytes=Base.decode64!(term["bytes"]);bits=term["bits"];<<v::bitstring-size(bits),_::bitstring>>=bytes;v
    "tuple" -> term["items"] |> Enum.map(get) |> List.to_tuple()
    "nil" -> []
    "list" -> Enum.reduce(Enum.reverse(term["items"]),get.(term["tail"]),fn id,tail->[get.(id)|tail]end)
    "map" -> Map.new(term["entries"],fn [k,v]->{get.(k),get.(v)}end)
   end
   :array.set(:array.size(array),value,array)
  end)
 end
 def core(module,array) do
  nodes=List.to_tuple(module["nodes"]);root=elem(nodes,module["core"])
  literal=fn id -> node=elem(nodes,id);:cerl.set_ann(:cerl.abstract(:array.get(node["value"],array)),:array.get(node["annotations"],array)) end
  attrs=Enum.at(root["groups"],2)|>Enum.chunk_every(2)|>Enum.map(fn [k,v]->{literal.(k),literal.(v)}end)
  :cerl.c_module(:cerl.c_atom(String.to_atom(module["name"])),[],attrs,[])
 end
 def missing(acc \\ []) do
  receive do {_,:ext_types,description}->missing([description|acc]) after 0->Enum.reverse(acc) end
 end
 def result(fun) do
  try do %{status: "ok",value: fun.(),missing: missing()}
  rescue e -> %{status: "error",class: Atom.to_string(e.__struct__),message: Exception.message(e),missing: missing()}
  catch kind,reason -> %{status: "error",class: Atom.to_string(kind),reason: NativeTypes.term(reason),missing: missing()}
  end
 end
 def environment(path) do
  stat=File.stat!(path)
  key={__MODULE__,:environment,path,stat.size,stat.mtime}
  case Process.get(key) do
   nil ->
    data=path|>File.read!()|>:json.decode()
    value={CoreExport.rebuild_terms(data["type_tables"]),CoreExport.rebuild_terms(data["exported_types"])}
    Process.put(key,value)
    value
   value -> value
  end
 end
 def run(request) do
  data=request["core"]|>File.read!()|>:json.decode();modules=data["modules"]
  decoded=Enum.map(modules,&decode/1);cores=Enum.zip_with(modules,decoded,&core/2)
  tables=:ets.new(:forms_types,[:set]);exports=:ets.new(:forms_exports,[:set])
  if request["environment"] do
   {type_tables,exported_types}=environment(request["environment"])
   targets=MapSet.new(Enum.map(modules,&String.to_atom(&1["name"])))
   for {name,table}<-:maps.to_list(type_tables),not MapSet.member?(targets,name),do: :ets.insert(tables,{name,table})
   for {name,_,_}=entry<-exported_types,not MapSet.member?(targets,name),do: :ets.insert(exports,{entry})
  end
  environments=Enum.zip_with(modules,cores,fn module,core->
   name=String.to_atom(module["name"])
   case :dialyzer_utils.get_record_and_type_info(core) do
    {:ok,table}->:ets.insert(tables,{name,table}); %{module: name,table: table,status: "ok"}
    {:error,error}->%{module: name,status: "error",message: List.to_string(error)}
   end
  end)
  Enum.zip(modules,decoded)|>Enum.each(fn {module,array}->
   forms=:array.get(module["source_forms"],array)
   for {:attribute,_,:export_type,types}<-forms, {name,arity}<-types,do: :ets.insert(exports,{{String.to_atom(module["name"]),name,arity}})
  end)
  rows=Enum.map(request["nodes"],fn root->
   module=Enum.at(modules,root["module"]);array=Enum.at(decoded,root["module"])
   form=:array.get(root["raw"],array);name=String.to_atom(module["name"])
   native=result(fn ->
    site={:spec,{name,:__inventory__,0},"inventory"}
    case request["mode"] do
     "without_remote" -> :erl_types.t_from_form_without_remote(form,site,:ets.lookup_element(tables,name,2))
     "check_remote" -> :erl_types.t_from_form_check_remote(form,exports,site,tables);:erl_types.t_atom(:ok)
     "check_fields" -> :erl_types.t_check_record_fields(form,exports,site,tables,:erl_types.var_table__new(),:erl_types.cache__new());:erl_types.t_atom(:ok)
     _ -> {type,_}=:erl_types.t_from_form(form,exports,site,tables,:erl_types.var_table__new(),:erl_types.cache__new());type
    end
   end)
   Map.merge(Map.take(root,["id","module","raw","kind"]),native)
  end)
  :ets.delete(tables);:ets.delete(exports)
  types=CoreExport.export_terms(Enum.map(rows,&Map.get(&1,:value,[])))
  missing=CoreExport.export_terms(Enum.map(rows,& &1.missing))
  rows=Enum.map(rows,&Map.drop(&1,[:value,:missing]))
  %{version: 3,types: types,missing: missing,inspection: request["inspection"],roots: rows,environments: Enum.map(environments,&Map.drop(&1,[:table]))}
 end
end
