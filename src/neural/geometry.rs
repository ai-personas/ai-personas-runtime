//! Install and run the published OpenSCAD compiler against exact project files.
pub mod history;
mod member;
pub mod profile;
use super::{failure, operations, store, workspace::{byte_digest, controlled}, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::Connection;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, io::Cursor, process::Stdio};

const PACKAGE: &str = "openscad-wasm-0.0.4";
const URL: &str = "https://cdn.jsdelivr.net/npm/openscad-wasm@0.0.4/openscad.js";
const MODULE_SHA256: &str = "129a861c3acc1070ea88d8806fa3c1f286fc5a6cb96d9252fd6e4f27a88079ab";
const WORKER: &str = include_str!("openscad_worker.mjs");

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Install {
    /// Optional exact asset or object containing the published openscad.js from openscad-wasm 0.0.4. Omit to retrieve the pinned public release.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Render {
    /// Exact OpenSCAD source asset revision, with its logical project path.
    pub reference: String,
    /// Exact companion assets for include, use, import or surface. Their library paths form the project tree.
    #[serde(default)]
    pub companions: Vec<String>,
    /// Overrides for named OpenSCAD variables. Values may be strings, finite numbers, booleans or vectors of these literals.
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json_raw::Value>")]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {
    /// Optional delivery or request revision. Checks membership and the original file write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// Exact asset revision or byte object. For a ZIP, select the STL member.
    pub reference: String,
    /// Original case-sensitive path inside the referenced ZIP; omit for a standalone STL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member: Option<String>,
}

fn paths(db: &Connection, input: &Render) -> Result<BTreeMap<std::path::PathBuf, String>> {
    fn literal(value: &Value) -> bool {
        match value {
            Value::Bool(_) | Value::Number(_) | Value::String(_) => true,
            Value::Array(values) => values.iter().all(literal),
            _ => false,
        }
    }
    for (name, value) in &input.parameters {
        let mut chars = name.chars();
        if !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            || !literal(value)
        {
            return Err(failure(
                "Use OpenSCAD variable names and literal parameter values",
            ));
        }
    }
    let references: Vec<_> = std::iter::once(&input.reference)
        .chain(&input.companions)
        .cloned()
        .collect();
    let paths = operations::package_paths(db, &references)?;
    if paths
        .keys()
        .any(|p| p.as_os_str().as_encoded_bytes().contains(&0))
    {
        return Err(failure("Project paths cannot contain a null byte"));
    }
    Ok(paths)
}

pub(super) fn validate_render(db: &Connection, input: &Render) -> Result<()> {
    paths(db, input).map(|_| ())
}

pub(super) fn validate_install(db: &Connection, input: &Install) -> Result<()> {
    if let Some(source) = &input.source {
        if byte_digest(db, source)? != MODULE_SHA256 {
            return Err(failure(
                "Compiler bytes differ from the pinned openscad-wasm 0.0.4 release",
            ));
        }
    }
    Ok(())
}

async fn worker(
    compiler: &[u8],
    mut request: Value,
    stopped: &mut impl FnMut() -> bool,
) -> Result<Value> {
    use tokio::io::AsyncWriteExt;
    let node = super::workspace::node_executable()?;
    request["compiler"] = json!(STANDARD.encode(compiler));
    let payload = serde_json::to_vec(&request)?;
    drop(request);
    controlled(
        async {
            let mut child = tokio::process::Command::new(&node)
                .env_clear()
                .args(["--permission", "--input-type=module", "-e", WORKER])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| failure("OpenSCAD worker input unavailable"))?;
            let send = async move { stdin.write_all(&payload).await };
            let (sent, output) = tokio::join!(send, child.wait_with_output());
            let output = output?;
            if !output.status.success() {
                return Err(failure(format!(
                    "OpenSCAD worker exited with {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                )));
            }
            sent?;
            let mut report: Value = serde_json::from_slice(&output.stdout)?;
            report["host_executable"] = json!(node);
            Ok(report)
        },
        stopped,
    )
    .await
}

fn existing(db: &Connection) -> Result<Option<Value>> {
    let Some(record) = store::head(db, "installation", PACKAGE)? else {
        return Ok(None);
    };
    if record.body["module"] != format!("object:{MODULE_SHA256}") {
        return Err(failure(
            "Installed compiler identity differs from its package",
        ));
    }
    Ok(Some(
        json!({"installation":record.reference.handle(),"record":record.body}),
    ))
}

pub async fn install(
    root: &std::path::Path,
    actor: &str,
    invocation: Option<&str>,
    input: &Install,
    mut stopped: impl FnMut() -> bool,
) -> Result<Value> {
    let mut store = store::Store::open(root)?;
    validate_install(&store.db, input)?;
    if let Some(mut installed) = existing(&store.db)? {
        store::bytes(&store.db, MODULE_SHA256)?;
        installed["already_installed"] = json!(true);
        return Ok(installed);
    }
    let (module, source) = if let Some(reference) = &input.source {
        (store::bytes(&store.db, MODULE_SHA256)?, reference.as_str())
    } else {
        let bytes = controlled(
            async {
                Ok(reqwest::Client::new()
                    .get(URL)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?
                    .to_vec())
            },
            &mut stopped,
        )
        .await?;
        (bytes, URL)
    };
    if crate::store::sha256_bytes(&module) != MODULE_SHA256 {
        return Err(failure(
            "Downloaded compiler differs from its published package digest",
        ));
    }
    let probe = worker(&module, json!({"version":true}), &mut stopped).await?;
    let version = probe["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|m| m.starts_with("OpenSCAD version "));
    if probe["exit_code"] != 0 || version.is_none() {
        return Err(failure(format!("OpenSCAD version probe failed: {probe}")));
    }
    store.transaction(|db| {
        if let Some(mut installed)=existing(db)? {installed["already_installed"]=json!(true);return Ok(installed);}
        let digest=store::put_bytes(db,"text/javascript",&module)?;
        let body=json!({"package":"openscad-wasm","release":"0.0.4","module":format!("object:{digest}"),"module_bytes":module.len(),
            "source":source,"published_module":URL,"license":"GPL-2.0","installed_by":actor,"installed_at":super::now(),"origin":if invocation.is_some() {"persona"} else {"operator"},"invocation":invocation,
            "compiler_version":version,"host_at_install":probe["node"],"host_executable":probe["host_executable"],"probe":probe});
        let r=store::write(db,"installation",PACKAGE,0,&body,invocation)?;
        Ok(json!({"installation":r.handle(),"already_installed":false,"record":body}))
    })
}

fn read_mesh(bytes: &[u8]) -> Result<(stl_io::IndexedMesh,[f64;3],[f64;3])> {
    let mesh = stl_io::read_stl(&mut Cursor::new(bytes))?;
    if mesh.faces.is_empty() {
        return Err(failure("STL contains no triangles"));
    }
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in &mesh.vertices {
        for axis in 0..3 {
            let v = f64::from(point[axis]);
            if !v.is_finite() {
                return Err(failure("STL contains a nonfinite vertex"));
            }
            min[axis] = min[axis].min(v);
            max[axis] = max[axis].max(v);
        }
    }
    Ok((mesh,min,max))
}

pub fn measure(bytes: &[u8]) -> Result<Value> {
    let (mesh,min,max)=read_mesh(bytes)?;
    let span: [f64; 3] = std::array::from_fn(|axis| max[axis] - min[axis]);
    let edge_check = mesh.validate();
    Ok(
        json!({"triangles":mesh.faces.len(),"distinct_vertices":mesh.vertices.len(),"minimum":min,"maximum":max,"span":span,
        "units":"STL coordinates have no encoded unit","stl_io_0_8_5_validation":{"passed":edge_check.is_ok(),"diagnostic":edge_check.err().map(|e|e.to_string())}}),
    )
}

pub async fn inspect(root: &std::path::Path, input: Inspect) -> Result<Value> {
    let root = root.to_owned();
    tokio::task::spawn_blocking(move||{
        let mut store=store::Store::open(&root)?;
        let source = store.snapshot(|db| member::MeshInput::read(db, &input.reference, input.member.as_deref(), input.edition.as_deref()))?;
        let geometry = measure(&source.bytes)?;
        let object = format!("object:{}", source.digest);
        let lookup = if source.member.is_some() { &object } else { &input.reference };
        let mut result = json!({"reference":input.reference,"mesh":object,"bytes":source.bytes.len(),"geometry":geometry});
        if let Some(edition) = input.edition { result["package_edition"] = json!(edition); }
        let finish = |db: &rusqlite::Transaction<'_>| -> Result<_> {
            source.retain(db, &mut result)?;
            result["render_history"] = history::lookup(db, &history::Lookup::first(lookup))?;
            Ok(result)
        };
        if source.member.is_some() { store.transaction(finish) } else { store.snapshot(finish) }
    }).await?
}

pub async fn render(
    root: &std::path::Path,
    input: &Render,
    mut stopped: impl FnMut() -> bool,
) -> Result<Value> {
    let mut store = store::Store::open(root)?;
    let installed = existing(&store.db)?.ok_or_else(|| {
        failure("Install the workspace compiler with geometry.install before rendering")
    })?;
    let module = store::bytes(&store.db, MODULE_SHA256)?;
    let mut files = Vec::new();
    let mut supplied = Vec::new();
    let mut entry = None;
    for (path, reference) in paths(&store.db, input)? {
        let digest = byte_digest(&store.db, &reference)?;
        let bytes = store::bytes(&store.db, &digest)?;
        if reference == input.reference {
            entry = Some(path.clone());
        }
        files.push(json!({"path":path,"bytes":STANDARD.encode(&bytes)}));
        supplied.push(json!({"path":path,"reference":reference,"content":format!("object:{digest}"),"bytes":bytes.len()}));
    }
    let entry = entry.ok_or_else(|| failure("Missing project entry"))?;
    let mut run = worker(
        &module,
        json!({"entry":entry,"files":files,"parameters":input.parameters}),
        &mut stopped,
    )
    .await?;
    let output = match run.as_object_mut().and_then(|r| r.remove("mesh")) {
        Some(Value::String(encoded)) => {
            let bytes = STANDARD.decode(encoded)?;
            Some(
                controlled(
                    async {
                        Ok(tokio::task::spawn_blocking(move || {
                            let geometry = measure(&bytes);
                            (bytes, geometry)
                        })
                        .await?)
                    },
                    &mut stopped,
                )
                .await?,
            )
        }
        None => None,
        _ => return Err(failure("Invalid mesh payload from the installed compiler")),
    };
    let messages = run["messages"]
        .as_array()
        .ok_or_else(|| failure("Missing compiler diagnostics"))?;
    let diagnostic = messages
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    let warnings = messages
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| s.starts_with("WARNING:") || s.starts_with("EXPORT-WARNING:"))
        .collect::<Vec<_>>();
    let errors = messages
        .iter()
        .filter_map(Value::as_str)
        .filter(|s| s.starts_with("ERROR:") || s.starts_with("Parser error"))
        .collect::<Vec<_>>();
    store.transaction(|db| {
        let log=store::put_bytes(db,"text/plain; charset=utf-8",diagnostic.as_bytes())?;
        let dependencies=run["dependencies"].as_str().map(|d|store::put_bytes(db,"text/plain; charset=utf-8",d.as_bytes())).transpose()?;
        let mut result=json!({"source":input.reference,"entry":entry,"files_supplied":supplied,"parameters":input.parameters,
            "installation":installed["installation"],"compiler_module":format!("object:{MODULE_SHA256}"),"compiler_version":installed["record"]["compiler_version"],
            "host":run["node"],"host_executable":run["host_executable"],"worker_sha256":crate::store::sha256_bytes(WORKER.as_bytes()),
            "node_options":run["node_options"],"environment_keys":run["environment_keys"],"compiler_arguments":run["compiler_arguments"],
            "exit_code":run["exit_code"],"worker_error":run["error"],"diagnostics":format!("object:{log}"),"warnings":warnings,"errors":errors,
            "dependencies":dependencies.map(|d|format!("object:{d}")),"success":false});
        if let Some((bytes,geometry))=output {
            let digest=store::put_bytes(db,"model/stl",&bytes)?;
            result["mesh"]=json!(format!("object:{digest}"));result["bytes"]=json!(bytes.len());
            match geometry {
                Ok(geometry)=>{result["geometry"]=geometry;result["success"]=json!(run["exit_code"]==0 && run["error"].is_null() && errors.is_empty());}
                Err(error)=>result["mesh_error"]=json!(error.to_string()),
            }
        }
        Ok(result)
    })
}
