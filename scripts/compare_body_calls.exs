[root,native,rust,out]=System.argv()
Code.require_file(Path.join(root,"scripts/type_codec.exs"));Code.require_file(Path.join(root,"scripts/type_wire.exs"));Code.require_file(Path.join(root,"scripts/core_export.exs"))
defmodule CallCompare do
 def norm(v),do: v|>TypeCodec.type()|>NativeTypeWire.encode()
 def snap(s) do
  s|>Map.update!("entries",fn es->Map.new(es,fn {k,v}->{k,v|>Map.update!("args",&Enum.map(&1,fn t->norm(t)end))|>Map.update!("result",&norm/1)}end)end)|>Map.update!("environments",fn es->Map.new(es,fn {k,v}->{k,Map.new(v,fn {key,t}->{key,norm(t)}end)}end)end)
 end
 def success(n,r) do
  checks=%{type: norm(n["type"])==norm(r["type"]),map: n["map"]|>Map.update!("types",fn ts->Map.new(ts,fn {k,t}->{k,norm(t)}end)end)==r["map"]|>Map.update!("types",fn ts->Map.new(ts,fn {k,t}->{k,norm(t)}end)end),functions: snap(n["functions"])==snap(r["functions"]),warnings: r["warnings_match"]==true}
  {Enum.all?(Map.values(checks)),checks}
 end
 def compare(n,r) do
  if n["status"]=="error" do
   reason=try do CoreExport.rebuild_terms(r["error"]["reason"]) rescue _->:unparsed end
   {r["status"]=="error" and n["kind"]==r["error"]["kind"] and reason===CoreExport.rebuild_terms(n["reason"]),%{error: r}}
  else
   if r["status"]=="ok",do: success(n,r),else: {false,%{status: r}}
  end
 end
end
ns=File.stream!(native)|>Enum.map(&:json.decode/1);rs=File.stream!(rust)|>Enum.map(&:json.decode/1)
if length(ns)!=length(rs),do: raise("count mismatch #{length(ns)} vs #{length(rs)}")
failures=Enum.zip(ns,rs)|>Enum.flat_map(fn {n,r}->{ok,detail}=CallCompare.compare(n["native"],r);if ok,do: [],else: [%{id: n["id"],detail: detail}]end)
r=%{cases: length(ns),matches: length(ns)-length(failures),native_errors: Enum.count(ns,&(&1["native"]["status"]=="error")),failures: failures};File.write!(out,:json.encode(r));IO.inspect(Map.delete(r,:failures));if failures != [],do: System.halt(1)
