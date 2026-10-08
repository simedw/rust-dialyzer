Code.require_file("../core_export.exs", __DIR__)
defmodule PreparationBoundaries do
 def write(core,forms,name,environment,dir) do
  path=Path.join(dir,"#{name}.core.json")
  module=CoreExport.export_module(:cerl.concrete(:cerl.module_name(core)),forms,core)
  File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [module]}))
  %{id: name,core: Path.expand(path),environment: Path.expand(environment)}
 end
 def run(base_environment,dir) do
  File.mkdir_p!(dir)
  source=Path.join(dir,"prep_boundary.erl")
  File.write!(source,"-module(prep_boundary).\n-export([f/1]).\n-spec f(A) -> A when A :: integer().\nf(A) -> A.\n")
  {:ok,:prep_boundary}=:compile.file(String.to_charlist(source),[:debug_info,{:outdir,String.to_charlist(dir)}])
  beam=Path.join(dir,"prep_boundary.beam")
  {:ok,{:prep_boundary,[debug_info: {:debug_info_v1,backend,metadata},abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(String.to_charlist(beam),[:debug_info,:abstract_code])
  {:ok,unclean}=backend.debug_info(:core_v1,:prep_boundary,metadata,[:no_copt,:to_core,:binary,:return_errors,:no_inline])
  core=:dialyzer_clean_core.clean(unclean)
  var=fn v->{:var,3,v}end
  typ=fn n->{:type,3,n,[]}end
  fun={:type,3,:fun,[{:type,3,:product,[var.(:A)]},var.(:A)]}
  valid_constraint=fn lhs,rhs->{:type,3,:constraint,[{:atom,3,:is_subtype},[lhs,rhs]]}end
  cases=[
   {"valid_atom_variable",[valid_constraint.(var.(:A),typ.(:integer))]},
   {"valid_integer_variable",[valid_constraint.(var.(77),typ.(:integer))]},
   {"valid_annotated_variable",[valid_constraint.({:ann_type,3,[var.(:Annotation),var.(:A)]},typ.(:integer))]},
   {"underscore_lhs",[valid_constraint.(var.(:_),typ.(:integer))]},
   {"atom_any_lhs",[valid_constraint.(typ.(:any),typ.(:integer))]},
   {"integer_lhs",[valid_constraint.(typ.(:integer),typ.(:integer))]},
   {"atom_singleton_lhs",[valid_constraint.({:atom,3,:hello},typ.(:integer))]},
   {"tuple_lhs",[valid_constraint.({:type,3,:tuple,[typ.(:integer),typ.(:atom)]},typ.(:integer))]},
   {"function_lhs",[valid_constraint.({:type,3,:fun,[{:type,3,:product,[typ.(:integer)]},typ.(:atom)]},typ.(:integer))]},
   {"unknown_guard",[{:type,3,:constraint,[{:atom,3,:unknown_guard},[var.(:A),typ.(:integer)]]}]},
   {"quoted_guard",[{:type,3,:constraint,[{:atom,3,:"Unusual guard"},[var.(:A)]]}]},
   {"empty_subtype",[{:type,3,:constraint,[{:atom,3,:is_subtype},[]]}]},
   {"triple_subtype",[{:type,3,:constraint,[{:atom,3,:is_subtype},[var.(:A),typ.(:integer),typ.(:atom)]]}]},
   {"non_form_constraint",[37]},
   {"non_form_lhs",[valid_constraint.(37,typ.(:integer))]},
   {"non_form_rhs",[valid_constraint.(var.(:A),37)]},
   {"nonvar_then_guard",[valid_constraint.(typ.(:integer),typ.(:integer)),{:type,3,:constraint,[{:atom,3,:unknown_guard},[]]}]},
   {"nonvar_then_malformed_lhs",[valid_constraint.(typ.(:integer),typ.(:integer)),valid_constraint.(37,typ.(:integer))]},
   {"malformed_rhs_then_guard",[valid_constraint.(var.(:A),37),{:type,3,:constraint,[{:atom,3,:unknown_guard},[]]}]}
  ]
  rows=for {name,constraints}<-cases do
   spec={{:f,1},[{:type,3,:bounded_fun,[fun,constraints]}]}
   source_forms=Enum.map(forms,fn {:attribute,a,:spec,_}->{:attribute,a,:spec,spec};other->other end)
   attrs=Enum.map(:cerl.module_attrs(core),fn {key,value}->if :cerl.concrete(key)==:spec,do: {key,:cerl.set_ann(:cerl.abstract([spec]),:cerl.get_ann(value))},else: {key,value}end)
   changed=:cerl.update_c_module(core,:cerl.module_name(core),:cerl.module_exports(core),attrs,:cerl.module_defs(core))
   write(changed,source_forms,name,base_environment,dir)
  end
  File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn r->[:json.encode(r),"\n"]end))
 end
end
[env,dir]=System.argv();PreparationBoundaries.run(env,dir)
