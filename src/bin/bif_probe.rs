use rust_dialyzer_poc::bifs::boundary::NativeTerm;
use rust_dialyzer_poc::bifs::registry::{Mfa, Registry, TransferError};
use serde::Deserialize;
use serde_json::json;
use std::io::{self, BufRead};
#[derive(Deserialize)]
struct Request {
    mfa: Mfa,
    arguments: Vec<NativeTerm>,
    #[serde(default)]
    expected: Option<NativeTerm>,
    #[serde(default)]
    expected_error: Option<serde_json::Value>,
    #[serde(default)]
    id: Option<String>,
}
fn main() {
    let registry =
        Registry::standard().expect("valid native catalogue");
    if std::env::args().any(|x| x == "--supported") {
        println!(
            "{}",
            serde_json::to_string(&registry.supported_mfas()).unwrap()
        );
        return;
    }
    for line in io::stdin().lock().lines() {
        let response = (|| -> Result<serde_json::Value, String> {
            let request: Request = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let arguments = request
                .arguments
                .iter()
                .map(NativeTerm::type_value)
                .collect::<Result<Vec<_>, _>>()?;
            let result = match registry.transfer(&request.mfa, &arguments) {
                Ok(value) => value,
                Err(TransferError::NativeException(error)) => {
                    let actual = error.report();
                    let mut expected = request.expected_error;
                    if let Some(value) = expected
                        .as_mut()
                        .and_then(|v| v.get_mut("payload"))
                        .filter(|v| v.get("kind").and_then(|v| v.as_str()) == Some("type"))
                    {
                        let term: NativeTerm = serde_json::from_value(value["value"].clone())
                            .map_err(|e| e.to_string())?;
                        value["value"] =
                            serde_json::to_value(term.type_value()?).map_err(|e| e.to_string())?;
                    }
                    return Ok(
                        json!({"ok":false,"id":request.id,"native_error":actual,"parity":expected.as_ref().map(|e|e==&actual)}),
                    );
                }
                Err(error) => return Err(error.to_string()),
            };
            let expected = request
                .expected
                .as_ref()
                .map(NativeTerm::type_value)
                .transpose()?;
            Ok(
                json!({"ok":true,"id":request.id,"known":result.known,"runtime_builtin":result.runtime_builtin,"accepted":result.accepted,"result":result.result,"parity":expected.as_ref().map(|x|x==&result.result),"expected":expected}),
            )
        })();
        println!(
            "{}",
            match response {
                Ok(value) => value,
                Err(error) => json!({"ok":false,"error":error}),
            }
        );
    }
}
