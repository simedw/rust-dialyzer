use rust_dialyzer_poc::incremental::{Inputs, plan};
use std::io::{self, BufRead};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for line in io::stdin().lock().lines() {
        let input: Inputs = serde_json::from_str(&line?)?;
        println!("{}", serde_json::to_string(&plan(&input))?);
    }
    Ok(())
}
