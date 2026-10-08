for line<-IO.stream(:stdio,:line)do
 row=:json.decode(line);dict=Enum.reduce(row["insert"],:dict.new(),fn key,d->key=if is_binary(key),do: String.to_atom(key),else: key;:dict.store(key,true,d)end)
 fold=:dict.fold(fn key,_v,acc->[key|acc]end,[],dict)|>Enum.reverse()|>Enum.map(fn key->if is_atom(key),do: Atom.to_string(key),else: key end)
 keys=Enum.map(:dict.fetch_keys(dict),fn key->if is_atom(key),do: Atom.to_string(key),else: key end)
 IO.puts(:json.encode(%{id: row["id"],dict: keys,fold: fold,slots: [elem(dict,2),elem(dict,3),elem(dict,4)]}))
end
