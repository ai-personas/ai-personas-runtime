use super::{failure, relative, workspace, Invocation, Outcome, Result, STANDARD};
use base64::Engine as _;
use serde_json::{json, Value};
use wasmi::{ExternType, Linker, Store};
use wasmi_wasi::{
    wasi_common::pipe::{ReadPipe, WritePipe},
    Dir, WasiCtxBuilder,
};

pub(super) fn validate(bytes: &[u8]) -> Result<Value> {
    let (_, module) = crate::computation::compile(bytes).map_err(failure)?;
    if !matches!(module.get_export("_start"),Some(ExternType::Func(t)) if t.params().is_empty() && t.results().is_empty())
        || !matches!(module.get_export("memory"), Some(ExternType::Memory(_)))
    {
        return Err(failure(
            "A WASI command must export memory and _start() with no results",
        ));
    }
    let mut imports = Vec::new();
    for import in module.imports() {
        if import.module() != "wasi_snapshot_preview1"
            || !matches!(import.ty(), ExternType::Func(_))
        {
            return Err(failure(format!(
                "Unsupported command import {}.{}",
                import.module(),
                import.name()
            )));
        }
        imports.push(import.name());
    }
    Ok(
        json!({"kind":"static_declarations","interface":"wasi_snapshot_preview1","entry":"_start","imports":imports,
        "executed":false}),
    )
}

pub fn worker() -> Result<()> {
    let input: Invocation = serde_json::from_reader(std::io::stdin().lock())?;
    let dir = tempfile::tempdir_in(".")?;
    for (name, data) in &input.files {
        relative(name)?;
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, STANDARD.decode(data)?)?;
    }
    for name in &input.outputs {
        relative(name)?;
    }
    let bytes = STANDARD.decode(input.program)?;
    let (engine, module) = crate::computation::compile(&bytes).map_err(failure)?;
    let stdout = WritePipe::new_in_memory();
    let stderr = WritePipe::new_in_memory();
    let mut ctx = WasiCtxBuilder::new();
    ctx.arg(&input.name)?
        .args(&input.arguments)?
        .stdin(Box::new(ReadPipe::from(STANDARD.decode(input.stdin)?)))
        .stdout(Box::new(stdout.clone()))
        .stderr(Box::new(stderr.clone()))
        .preopened_dir(
            Dir::open_ambient_dir(dir.path(), wasmi_wasi::ambient_authority())?,
            ".",
        )?;
    let mut store = Store::new(&engine, ctx.build());
    let mut linker = Linker::new(&engine);
    wasmi_wasi::add_to_linker(&mut linker, |ctx| ctx)?;
    let result = linker
        .instantiate_and_start(&mut store, &module)
        .and_then(|instance| {
            instance
                .get_typed_func::<(), ()>(&store, "_start")?
                .call(&mut store, ())
        });
    drop(store);
    let (exit_code, error) = match result {
        Ok(()) => (Some(0), None),
        Err(e) if e.i32_exit_status().is_some() => (e.i32_exit_status(), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let mut report = Outcome {
        exit_code,
        error,
        stdout: STANDARD.encode(
            stdout
                .try_into_inner()
                .map_err(|_| failure("Command stdout still open"))?
                .into_inner(),
        ),
        stderr: STANDARD.encode(
            stderr
                .try_into_inner()
                .map_err(|_| failure("Command stderr still open"))?
                .into_inner(),
        ),
        host: json!({"runtime":"Wasmi","version":"2.0.0","interface":"wasi_snapshot_preview1","preopened_directory":".",
            "environment_keys":[],"build_sha256":crate::store::sha256_bytes(&[include_bytes!("wasi.rs").as_slice(),
                include_bytes!("../../computation.rs").as_slice(),include_bytes!("../../../Cargo.lock").as_slice()].concat())}),
        ..Default::default()
    };
    for name in input.outputs {
        match workspace::read_file(dir.path(), &name) {
            Ok((_, bytes)) => {
                report.files.insert(name, STANDARD.encode(bytes));
            }
            Err(e) => {
                report.file_errors.insert(name, e.to_string());
            }
        }
    }
    serde_json::to_writer(std::io::stdout().lock(), &report)?;
    Ok(())
}
