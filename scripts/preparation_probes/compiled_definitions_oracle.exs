Code.require_file("../forms_runtime.exs",__DIR__)
defmodule CompiledDefinitionsOracle do
 def run(r,{old,old_exports}) do
  data=r["core"]|>File.read!()|>:json.decode();modules=data["modules"];arrays=Enum.map(modules,&FormsOracle.decode/1);cores=Enum.zip_with(modules,arrays,&FormsOracle.core/2)
  if r["proof_core"] do
   proof=Enum.zip([modules,arrays,cores])|>Enum.map(fn {mod,a,c}->CoreExport.export_module(String.to_atom(mod["name"]),:array.get(mod["source_forms"],a),c)end)
   File.mkdir_p!(Path.dirname(r["proof_core"]));File.write!(r["proof_core"],:json.encode(%{version: 6,profile: CoreExport.profile(),modules: proof}))
  end
  names=Enum.map(modules,&String.to_atom(&1["name"]));targets=MapSet.new(names);cs=:dialyzer_codeserver.new();key=make_ref();Process.put(key,cs)
  try do
   for {m,t}<-:maps.to_list(old),not MapSet.member?(targets,m),do: :dialyzer_codeserver.store_temp_records(m,t,cs)
   for {m,c}<-Enum.zip(names,cores) do
    {:ok,t}=:dialyzer_utils.get_record_and_type_info(c);:dialyzer_codeserver.store_temp_records(m,t,cs)
    {:ok,specs,callbacks}=:dialyzer_utils.get_spec_info(m,c,t);:dialyzer_codeserver.store_temp_contracts(m,specs,callbacks,cs)
   end
   own=Enum.zip(names,arrays)|>Enum.flat_map(fn {m,a}->mod=Enum.find(modules,&(&1["name"]==Atom.to_string(m)));for {:attribute,_,:export_type,ts}<-:array.get(mod["source_forms"],a),{n,arity}<-ts,do: {m,n,arity} end)
   exports=Enum.reject(old_exports,fn {m,_,_}->MapSet.member?(targets,m)end)++own
   cs=:dialyzer_codeserver.finalize_exported_types(:sets.from_list(exports),cs);Process.put(key,cs)
   cs=:dialyzer_utils.process_record_remote_types(cs);Process.put(key,cs)
   cs=:dialyzer_contracts.process_contract_remote_types(cs);Process.put(key,cs)
   records=Enum.flat_map(names,fn m->
    for {{:record,n},{_file,variants}}<-:maps.to_list(:dialyzer_codeserver.lookup_mod_records(m,cs)),{_arity,fields}<-variants do
     ts=Enum.map(fields,&elem(&1,2));%{module: Atom.to_string(m),tag: Atom.to_string(n),field_names: Enum.map(fields,fn {n,_,_}->Atom.to_string(n)end),fields: Enum.map(ts,&NativeTypes.term/1),prototype: NativeTypes.term(:erl_types.t_tuple([:erl_types.t_atom(n)|ts]))}
    end
   end)
   contracts=:dialyzer_codeserver.get_contracts(cs)|>:dict.to_list()|>Enum.map(fn {mfa,{_loc,c,_flags}}->contract(mfa,c,false)end)
   callbacks=:dialyzer_codeserver.get_callbacks(cs)|>Enum.flat_map(fn
    {{_,_,_}=mfa,{_loc,c,_flags}}->[contract(mfa,c,true)]
    _->[]
   end)
   %{status: "ok",records: records,contracts: contracts++callbacks,tables: Map.new(names,fn m->{Atom.to_string(m),CoreExport.export_terms(:dialyzer_codeserver.lookup_mod_records(m,cs))}end),events: Enum.map(:lists.usort(FormsOracle.missing()),&CoreExport.export_terms/1)}
  catch k,e->%{status: "error",kind: Atom.to_string(k),reason: CoreExport.export_terms(e),events: Enum.map(FormsOracle.missing(),&CoreExport.export_terms/1)}
  after :dialyzer_codeserver.delete(Process.delete(key)) end
 end
 def contract({m,f,a},c,callback),do: %{mfa: %{module: Atom.to_string(m),function: Atom.to_string(f),arity: a},callback: callback,capture: CoreExport.export_terms(c)}
end
:logger.set_primary_config(:level,:none)
{:ok,supervisor}=Task.Supervisor.start_link()
for line<-IO.stream(:stdio,:line) do
 request=:json.decode(line)
 imported=FormsOracle.environment(request["environment"])
 task=Task.Supervisor.async_nolink(supervisor,fn ->CompiledDefinitionsOracle.run(request,imported) end)
 result=case Task.yield(task,60_000)||Task.shutdown(task,:brutal_kill) do
  {:ok,result}->result
  {:exit,{reason,stack}=exit} when is_list(stack)->%{status: "error",kind: "error",reason: CoreExport.export_terms(reason),events: [],native_failure_boundary: "linked_worker_exit",native_process_exit_etf: Base.encode64(:erlang.term_to_binary(exit))}
  {:exit,reason}->%{status: "error",kind: "exit",reason: CoreExport.export_terms(reason),events: [],native_failure_boundary: "isolated_task_exit"}
  nil->%{status: "timeout",native_failure_boundary: "whole_native_case_60000ms"}
 end
 IO.puts(:json.encode(result))
end
