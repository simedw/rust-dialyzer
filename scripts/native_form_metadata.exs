# Native-only lever: expose pinned private declaration preprocessing without replacing OTP modules.
defmodule NativeFormMetadata do
 def profile do
  path=:code.which(:dialyzer_utils)|>List.to_string()
  Map.merge(CoreExport.profile(),%{metadata_beam_sha256: :crypto.hash(:sha256,File.read!(path))|>Base.encode16(case: :lower)})
 end
 def load do
  source=:code.lib_dir(:dialyzer)|>List.to_string()|>Path.join("src/dialyzer_utils.erl")
  {:ok,{:dialyzer_utils,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_utils),[:abstract_code])
  forms=Enum.map(forms,fn
   {:attribute,line,:module,:dialyzer_utils}->{:attribute,line,:module,:forms_native_utils}
   form->form
  end)
  case :compile.forms(forms,[:binary,:export_all,:return_errors,:return_warnings]) do
   {:ok,:forms_native_utils,binary,_warnings}-> {:module,:forms_native_utils}=:code.load_binary(:forms_native_utils,String.to_charlist(source),binary)
   failure->raise "native metadata lever compile failed: #{inspect(failure)}"
  end
 end
 def prepare(modules,cores,decoded,environment) do
  if :code.is_loaded(:forms_native_utils)==false,do: load()
  cs=:dialyzer_codeserver.new()
  targets=Enum.map(modules,&String.to_atom(&1["name"]))
  {base_tables,base_exports}=if environment,do: FormsOracle.environment(environment),else: {%{},[]}
  targetset=MapSet.new(targets)
  for {name,table}<-:maps.to_list(base_tables),not MapSet.member?(targetset,name),do: :dialyzer_codeserver.store_temp_records(name,table,cs)
  ownexports=Enum.zip(modules,decoded)|>Enum.flat_map(fn {module,array}->
   for {:attribute,_,:export_type,types}<-:array.get(module["source_forms"],array),{name,arity}<-types,do: {String.to_atom(module["name"]),name,arity}
  end)
  exported=Enum.reject(base_exports,fn {name,_,_}->MapSet.member?(targetset,name)end)++ownexports
  cs=:dialyzer_codeserver.finalize_exported_types(:sets.from_list(exported),cs)
  Enum.zip(modules,cores)|>Enum.each(fn {module,core}->{:ok,table}=:dialyzer_utils.get_record_and_type_info(core);:dialyzer_codeserver.store_temp_records(String.to_atom(module["name"]),table,cs)end)
  exports=:dialyzer_codeserver.get_exported_types_table(cs)
  apply(:forms_native_utils,:process_opaque_types0,[targets,cs,exports])
  for name<-targets do
   messages=:dialyzer_utils.process_record_remote_types_module(name,cs)
   for {:error,_}=error<-messages,do: throw(error)
   for {_,:ext_types,description}<-messages,do: send(self(),{self(),:ext_types,description})
  end
  apply(:forms_native_utils,:check_record_fields,[targets,cs,exports])
  {cs,:dialyzer_codeserver.get_temp_records_table(cs),exports}
 end
end
