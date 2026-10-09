use crate::core::ErlangTerms;
use crate::term_order::CaptureEnvironment;
use serde_json::Value;
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub struct LocalClosure {
    pub module: String,
    pub new_index: u32,
    pub old_uniq: u32,
    pub arity: usize,
    pub name: String,
    pub environment: CaptureEnvironment,
}
#[derive(Clone, Debug)]
pub enum ClosureOrderKey {
    Local(LocalClosure),
    External {
        module: String,
        function: String,
        arity: usize,
    },
}
impl ClosureOrderKey {
    pub fn parse(value: &Value) -> Result<Self, String> {
        let text = |key: &str| {
            value[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("closure.{key}: atom expected"))
        };
        let unsigned = |key: &str| {
            value[key]
                .as_u64()
                .ok_or_else(|| format!("closure.{key}: nonnegative integer expected"))
        };
        if value["kind"] == "external" {
            return Ok(Self::External {
                module: text("module")?,
                function: text("function")?,
                arity: usize::try_from(unsigned("arity")?)
                    .map_err(|_| "closure.arity: too large")?,
            });
        }
        if value.get("kind").is_some_and(|k| k != "local") {
            return Err("closure.kind: local or external expected".into());
        }
        let environment = if value.get("captures").is_some() {
            CaptureEnvironment::parse(&value["captures"])?
        } else {
            CaptureEnvironment::literals(&ErlangTerms::parse(&value["env_terms"])?)?
        };
        let captured_count = environment.len();
        if value.get("env_count").is_some() && unsigned("env_count")? != captured_count as u64 {
            return Err("closure.env_count: captured environment count mismatch".into());
        }
        Ok(Self::Local(LocalClosure {
            module: text("module")?,
            new_index: u32::try_from(unsigned("new_index")?)
                .map_err(|_| "closure.new_index: too large")?,
            old_uniq: u32::try_from(unsigned("uniq")?).map_err(|_| "closure.uniq: too large")?,
            arity: usize::try_from(unsigned("arity")?).map_err(|_| "closure.arity: too large")?,
            name: text("name")?,
            environment,
        }))
    }
    /// Whether comparing two keys that both satisfy this cannot fail.
    pub fn compares_infallibly(&self) -> bool {
        match self {
            Self::Local(closure) => closure.environment.is_native(),
            Self::External { .. } => true,
        }
    }
    pub fn compare(&self, other: &Self) -> Result<Ordering, String> {
        match (self, other) {
            (Self::Local(a), Self::Local(b)) => {
                let identity = a
                    .module
                    .cmp(&b.module)
                    .then(a.new_index.cmp(&b.new_index))
                    .then(a.old_uniq.cmp(&b.old_uniq))
                    .then(a.environment.len().cmp(&b.environment.len()));
                if identity != Ordering::Equal {
                    return Ok(identity);
                }
                a.environment.compare(&b.environment)
            }
            (
                Self::External {
                    module: am,
                    function: af,
                    arity: aa,
                },
                Self::External {
                    module: bm,
                    function: bf,
                    arity: ba,
                },
            ) => Ok(am.cmp(bm).then(af.cmp(bf)).then(aa.cmp(ba))),
            (Self::Local(_), Self::External { .. }) => Ok(Ordering::Less),
            (Self::External { .. }, Self::Local(_)) => Ok(Ordering::Greater),
        }
    }
}

#[derive(Clone, Debug)]
struct ClosureCodeSite {
    new_index: u32,
    old_uniq: u32,
    free: usize,
    arity: usize,
}
#[derive(Clone, Debug)]
pub struct ClosureCodeProfile {
    module: String,
    pub beam_sha256: String,
    sites: std::collections::BTreeMap<String, ClosureCodeSite>,
}
impl ClosureCodeProfile {
    pub fn module_name(&self) -> &str {
        &self.module
    }
    pub fn parse(raw: &Value) -> Result<Self, String> {
        let module = raw["module"]
            .as_str()
            .filter(|x| !x.is_empty())
            .ok_or("closure profile module expected")?
            .to_owned();
        let beam_sha256 = raw["original_sha256"]
            .as_str()
            .filter(|x| x.len() == 64 && x.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("closure profile SHA256 expected")?
            .to_owned();
        let table = raw["fun_table"]
            .as_array()
            .ok_or("closure profile fun_table expected")?;
        let mut sites = std::collections::BTreeMap::new();
        for (index, row) in table.iter().enumerate() {
            let field = |key: &str| {
                row[key]
                    .as_u64()
                    .ok_or_else(|| format!("closure profile [{index}].{key}: integer expected"))
            };
            let name = row["name"]
                .as_str()
                .filter(|x| !x.is_empty())
                .ok_or("closure profile function name expected")?
                .to_owned();
            let new_index =
                u32::try_from(field("new_index")?).map_err(|_| "closure new_index too large")?;
            if new_index as usize != index {
                return Err("closure profile new_index must equal FunT row position".into());
            }
            let compiled_arity =
                usize::try_from(field("arity")?).map_err(|_| "compiled arity too large")?;
            let free =
                usize::try_from(field("env_count")?).map_err(|_| "capture count too large")?;
            let arity = compiled_arity
                .checked_sub(free)
                .ok_or("compiled arity smaller than capture count")?;
            let site = ClosureCodeSite {
                new_index,
                old_uniq: u32::try_from(field("uniq")?).map_err(|_| "closure uniq too large")?,
                free,
                arity,
            };
            if sites.insert(name, site).is_some() {
                return Err("duplicate closure code-site name".into());
            }
        }
        Ok(Self {
            module,
            beam_sha256,
            sites,
        })
    }
    pub fn instantiate(
        &self,
        name: &str,
        environment: CaptureEnvironment,
    ) -> Result<ClosureOrderKey, String> {
        let site = self
            .sites
            .get(name)
            .ok_or_else(|| format!("unknown native closure code site {name}"))?;
        let captured_count = environment.len();
        if captured_count != site.free {
            return Err(format!(
                "closure {name}: expected {} captured values, got {captured_count}",
                site.free
            ));
        }
        Ok(ClosureOrderKey::Local(LocalClosure {
            module: self.module.clone(),
            new_index: site.new_index,
            old_uniq: site.old_uniq,
            arity: site.arity,
            name: name.to_owned(),
            environment,
        }))
    }
}
