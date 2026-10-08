[requests,actual]=System.argv()
r=File.stream!(requests)|>Enum.map(&:json.decode/1)
a=File.stream!(actual)|>Enum.map(&:json.decode/1)
true=length(r)==32;true=length(a)==32
for {r,a}<-Enum.zip(r,a) do
 true=r["id"]==a["id"]
 for {expected,actual}<-Enum.zip(r["expected"],a["fields"]),do: true=:erlang.binary_to_term(Base.decode16!(expected,case: :lower))===:erlang.binary_to_term(Base.decode16!(actual,case: :lower))
 IO.puts(:json.encode(%{id: r["id"],exact: true}))
end
