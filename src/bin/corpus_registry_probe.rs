use serde::Deserialize;
use serde_json::json;
use std::{
    fs,
    io::{self, BufRead},
    path::PathBuf,
};
#[derive(Deserialize)]
struct Request {
    id: String,
    suite_dir: PathBuf,
    declared: Vec<String>,
}
fn missing(request: &Request) -> Result<Vec<String>, String> {
    let directory = request.suite_dir.join("src");
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        if metadata.is_dir()
            || (metadata.is_file() && path.extension().is_some_and(|extension| extension == "erl"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    let mut names = paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("non-Unicode source filename")?;
            Ok(name.strip_suffix(".erl").unwrap_or(name).to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?;
    for declared in &request.declared {
        if let Some(index) = names.iter().position(|name| name == declared) {
            names.remove(index);
        }
    }
    Ok(names)
}
fn main() -> Result<(), String> {
    for line in io::stdin().lock().lines() {
        let request: Request =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{}", json!({"id":request.id,"missing":missing(&request)?}));
    }
    Ok(())
}
