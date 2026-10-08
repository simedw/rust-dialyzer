use rust_dialyzer_poc::native_runtime_hash::NativeHashKey;
use serde_json::{Value, json};
pub fn key(value: &Value) -> Result<NativeHashKey, String> {
    if let Some(fun) = value.get("external_fun") {
        let module = fun["module"]
            .as_str()
            .ok_or("external function module atom expected")?;
        let function = fun["function"]
            .as_str()
            .ok_or("external function name atom expected")?;
        if module.chars().count() > 255 || function.chars().count() > 255 {
            return Err("external function atom exceeds 255 characters".into());
        }
        let arity = fun["arity"]
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or("external function arity expected in 0..255")?;
        return Ok(NativeHashKey::ExternalFun {
            module: module.into(),
            function: function.into(),
            arity,
        });
    }
    if let Some(atom) = value["atom"].as_str() {
        return Ok(NativeHashKey::Atom(atom.into()));
    }
    if let Some(integer) = value["integer"].as_str() {
        return Ok(NativeHashKey::Integer(
            integer.parse().map_err(|_| "invalid integer")?,
        ));
    }
    if let Some(bits) = value["float"].as_u64() {
        return Ok(NativeHashKey::Float(bits));
    }
    if let Some(bytes) = value["bytes"].as_array() {
        let bytes = bytes
            .iter()
            .map(|x| {
                x.as_u64()
                    .and_then(|x| u8::try_from(x).ok())
                    .ok_or("invalid byte")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bits = value["bits"].as_u64().ok_or("missing bit length")?;
        return Ok(NativeHashKey::Bits { bytes, bits });
    }
    if let Some(pairs) = value["map"].as_array() {
        return Ok(NativeHashKey::Map(
            pairs
                .iter()
                .map(|x| Ok((key(&x[0])?, key(&x[1])?)))
                .collect::<Result<_, String>>()?,
        ));
    }
    if let Some(tuple) = value["tuple"].as_array() {
        return Ok(NativeHashKey::Tuple(
            tuple.iter().map(key).collect::<Result<_, _>>()?,
        ));
    }
    if value["nil"].as_bool() == Some(true) {
        return Ok(NativeHashKey::Nil);
    }
    if let Some(cons) = value["cons"].as_array()
        && cons.len() == 2
    {
        return Ok(NativeHashKey::Cons(
            Box::new(key(&cons[0])?),
            Box::new(key(&cons[1])?),
        ));
    }
    Err("unsupported primitive key".into())
}
pub fn wire(key: &NativeHashKey) -> Value {
    match key {
        NativeHashKey::ExternalFun {
            module,
            function,
            arity,
        } => json!({"external_fun":{"module":module,"function":function,"arity":arity}}),
        NativeHashKey::Atom(atom) => json!({"atom":atom}),
        NativeHashKey::Integer(value) => json!({"integer":value.to_string()}),
        NativeHashKey::Float(bits) => json!({"float":bits}),
        NativeHashKey::Bits { bytes, bits } => json!({"bytes":bytes,"bits":bits}),
        NativeHashKey::Map(pairs) => {
            json!({"map":pairs.iter().map(|(k,v)|vec![wire(k),wire(v)]).collect::<Vec<_>>()})
        }
        NativeHashKey::Tuple(elements) => {
            json!({"tuple":elements.iter().map(wire).collect::<Vec<_>>()})
        }
        NativeHashKey::Cons(head, tail) => json!({"cons":[wire(head),wire(tail)]}),
        NativeHashKey::Nil => json!({"nil":true}),
    }
}
