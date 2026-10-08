Code.require_file("../core_export.exs",__DIR__)
[dir]=System.argv();File.mkdir_p!(dir)
values=[:a,:"Unusual guard",:"λ",:"é",:"line\nnew",37,-78,1.5,[],[65,66],[0,255,256],[[65],[],[66]],<<65,255>>,<<206,187>>,[[65]|<<66>>],[65|37],{:a,[65]},%{a: 1,b: 2}]
rows=for t<-values,f<-["~w","~tw","~s","~ts","prefix:~w/~tw~n~~"] do
 args=if f=="prefix:~w/~tw~n~~",do: [t,t],else: [t]
 arena=CoreExport.export_terms(args)
 root=Enum.at(arena.terms,arena.root);arguments=if root.kind=="nil",do: [],else: root.items
 request=%{arena: arena,format: f,arguments: arguments,map_orders: (for {term,id}<-Enum.with_index(arena.terms),term.kind=="map",do: %{id: id,order: term.iterator_order})}
 result=try do %{ok: CoreExport.export_terms(:io_lib.format(String.to_charlist(f),args))} catch k,e->%{kind: Atom.to_string(k),reason: CoreExport.export_terms(e)} end
 {request,result}
end
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn {r,_}->[:json.encode(r),"\n"]end))
File.write!(Path.join(dir,"native.jsonl"),Enum.map(rows,fn {_,r}->[:json.encode(r),"\n"]end))
