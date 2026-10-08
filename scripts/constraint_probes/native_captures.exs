defmodule CaptureNative do
  def make(x), do: fn -> x end
  def make_other(x), do: fn _ -> x end
  def terms(t, state) do
    {node,state}=cond do
      is_function(t) ->
        info=Map.new(:erlang.fun_info(t))
        if info.type==:external do
          {%{kind: "function",flavor: "external",module: Atom.to_string(info.module),name: Atom.to_string(info.name),arity: info.arity},state}
        else
          {env,state}=children(info.env,state)
          {%{kind: "function",flavor: "local",module: Atom.to_string(info.module),name: Atom.to_string(info.name),arity: info.arity,new_index: info.new_index,uniq: info.uniq,environment: env},state}
        end
      is_reference(t) or is_pid(t) or is_port(t) ->
        {objects,next}=state.objects
        {object,objects,next}=case Map.fetch(objects,t) do
          {:ok,n}->{n,objects,next}
          :error->{next,Map.put(objects,t,next),next+1}
        end
        kind=cond do is_reference(t)->"reference";is_pid(t)->"pid";true->"port" end
        {%{kind: "runtime",runtime_kind: kind,scope: "native_pair",object: Integer.to_string(object)},%{state|objects: {objects,next}}}
      is_atom(t)->{%{kind: "atom",value: Atom.to_string(t)},state}
      is_integer(t)->{%{kind: "integer",value: Integer.to_string(t)},state}
      is_float(t)->{%{kind: "float",value: Base.encode16(<<t::float-size(64)>>)},state}
      is_tuple(t)->{ids,state}=children(Tuple.to_list(t),state);{%{kind: "tuple",items: ids},state}
      is_map(t)->
        {pairs,state}=Enum.map_reduce(:maps.to_list(t),state,fn {k,v},s->{k,s}=terms(k,s);{v,s}=terms(v,s);{[k,v],s} end)
        {%{kind: "map",entries: pairs},state}
      t==[]->{%{kind: "nil"},state}
      is_list(t)->
        {xs,tail}=split(t,[]);{xs,state}=children(xs,state);{tail,state}=terms(tail,state)
        {%{kind: "list",items: xs,tail: tail},state}
      is_binary(t)->{%{kind: "binary",value: Base.encode64(t)},state}
      is_bitstring(t)->n=bit_size(t);pad=rem(8-rem(n,8),8);{%{kind: "bitstring",bytes: Base.encode64(<<t::bitstring,0::size(pad)>>),bits: n},state}
    end
    id=state.next
    {id,%{state|next: id+1, terms: [node|state.terms]}}
  end
  def children(xs,state),do: Enum.map_reduce(xs,state,&terms/2)
  def split([h|t],acc),do: split(t,[h|acc])
  def split(t,acc),do: {Enum.reverse(acc),t}
  def encode(t,objects) do
    {root,state}=terms(t,%{next: 0,terms: [],objects: objects})
    {%{root: root,terms: Enum.reverse(state.terms)},state.objects}
  end
end
[r]=System.argv()
ref=make_ref()
leaf=[0,1,1.0,-0.0,0.0,9007199254740992,9007199254740992.0,:a,:b,:"é",{},[],[1|:tail],<<1::3>>,<<1>>,ref,self()]
funs=Enum.map(leaf,&CaptureNative.make/1)++Enum.map(leaf,&CaptureNative.make_other/1)++[Function.capture(:erlang,:abs,1),Function.capture(:"",:"",0)]
nested=Enum.map(funs,&CaptureNative.make/1)
values=leaf++funs++nested++Enum.flat_map(Enum.take(funs,8),fn f->[{f,ref},%{f=>:v},[f]] end)++[%{CaptureNative.make(1)=>:one,CaptureNative.make(1.0)=>:float},%{ref=>:v}]
ref2=make_ref()
lazy=[{{:a,ref},{:b,ref2}},{CaptureNative.make({:a,ref}),CaptureNative.make({:b,ref2})},{%{ref=>:a,ref2=>:b},%{ref=>:a}}]
comparisons=(for a<-values,b<-values,exact<-[false,true],do: {a,b,exact})++Enum.flat_map(lazy,fn {a,b}->[{a,b,false},{a,b,true},{b,a,false},{b,a,true}] end)
rows=for {a,b,exact}<-comparisons do
  {aa,objects}=CaptureNative.encode(a,{%{},0});{bb,_}=CaptureNative.encode(b,objects)
  {x,y}=if exact,do: {%{a=>:v},%{b=>:v}},else: {a,b}
  order=cond do x<y->-1;x>y->1;true->0 end
  %{capture_compare: %{a: aa,b: bb,exact: exact},native_order: order}
end
File.write!(r,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),values: length(values)})
