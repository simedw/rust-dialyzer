Code.require_file("../core_export.exs",__DIR__)
[dir]=System.argv();File.mkdir_p!(dir)
var=fn v->{:var,7,v}end;typ=fn t->{:type,7,t,[]}end
fun=fn args,ret->{:type,7,:fun,[{:type,7,:product,args},ret]}end
constraint=fn name,args->{:type,7,:constraint,[{:atom,7,name},args]}end
f=fun.([var.(:A)],var.(:A));g=fun.([typ.(:integer)],typ.(:atom));h=fun.([],typ.(:float))
cs=[constraint.(:is_subtype,[var.(:A),typ.(:integer)])]
forms=[[],[{f,[]}],[{f,[]},{g,[]}],[{f,cs}],[{f,cs},{g,[]}],[{f,[]},{g,cs}],[{f,cs},{g,cs},{h,[]}],[{f,[constraint.(:is_subtype,[var.(77),37])]}],[{f,[constraint.(:is_subtype,[var.(77),typ.(:integer)])]}],[{f,[constraint.(:foo,[typ.(:integer),typ.(:atom)])]}],[{f,[constraint.(:is_subtype,[])]}],[{f,[constraint.(77,37)]}],[{f,[constraint.(:foo,37)]}],[{f,[constraint.(:foo,[typ.(:integer)|37])]}],[{f,[constraint.(:foo,[37|37])]}],[{f,[37]}],[{f,[constraint.(:is_subtype,[{:ann_type,7,[var.(:Name),var.(:A)]},typ.(:integer)])]}],[{g,[constraint.(:foo,[var.(77)])]}],[{f,[]}|37],[{f,cs}|37],[{typ.(:integer),[]}],[{f,[]} ,37],[{f,[]} ,{g,[37]}],[{37,[constraint.(77,[])]}]]
rows=for form<-forms,op<-[:contract_to_string,:is_overloaded] do
 raw={:contract,[{:function,:constraints}],[],form}
 result=try do value=apply(:dialyzer_contracts,op,[raw]);%{ok: if(is_list(value),do: List.to_string(value),else: value)} catch k,e->%{kind: Atom.to_string(k),reason: CoreExport.export_terms(e)}end
 {%{op: Atom.to_string(op),arena: CoreExport.export_terms(raw)},result}
end
# Cardinality and malformed list tails do not inspect alternative element contents.
extra=for clauses<-[[],[37],[37,37],[37|78],[37,37|78],37,{37}],op<-[:is_overloaded,:contract_to_string] do
 raw={:contract,clauses,[],[{f,[]}]}
 result=try do value=apply(:dialyzer_contracts,op,[raw]);%{ok: if(is_list(value),do: List.to_string(value),else: value)}catch k,e->%{kind: Atom.to_string(k),reason: CoreExport.export_terms(e)}end
 {%{op: Atom.to_string(op),arena: CoreExport.export_terms(raw)},result}
end
rows=rows++extra
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn {r,_}->[:json.encode(r),"\n"]end));File.write!(Path.join(dir,"native.jsonl"),Enum.map(rows,fn {_,r}->[:json.encode(r),"\n"]end))
