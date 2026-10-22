//! WebAssembly computation without application resource quotas. This embedded
//! ABI supplies memory and run; host commands expose installed host runtimes.
use crate::store::sha256_bytes;
use serde::{Deserialize, Serialize};
use wasmi::{Config, Engine, Linker, Module, Store};

pub const PROFILE: &str = "compute.wasm.v1";
pub const OUTPUT_ESTIMATE_BYTES: usize = 8_192;

/// Bind each execution to the actual runner and resolved dependencies.
/// This describes the embedded engine's source build, not an OS isolation claim.
pub fn engine_revision() -> String {
    sha256_bytes(
        &[
            include_bytes!("computation.rs").as_slice(),
            include_bytes!("../Cargo.toml").as_slice(),
            include_bytes!("../Cargo.lock").as_slice(),
        ]
        .concat(),
    )
}

fn engine() -> Engine {
    let mut config = Config::default();
    config
        .consume_fuel(false)
        .allow_start_fn(true)
        .wasm_multi_memory(true)
        // Wasmi exposes numeric stack settings rather than an unbounded option.
        // Use machine representability, without a smaller application quota.
        .set_max_recursion_depth(usize::MAX)
        .set_max_stack_height(usize::MAX);
    Engine::new(&config)
}

/// Validation never instantiates or executes the program. No imports, including
/// WASI, filesystem, network, clocks, randomness or model calls, are linked.
pub fn validate(program: &str) -> Result<(), &'static str> {
    compile(program).map(|_| ())
}
fn compile(program: &str) -> Result<(Engine, Module), &'static str> {
    let engine = engine();
    let module = Module::new(&engine, program.as_bytes()).map_err(|_| "invalid_module")?;
    Ok((engine, module))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub succeeded: bool,
    /// Guest-authored output is data, never an instruction or proof of correctness.
    pub output: Option<String>,
    /// Fixed host codes avoid publishing parser diagnostics containing program text.
    pub failure: Option<String>,
    /// No fuel meter is enabled; absence must not be reported as zero use.
    pub fuel_consumed: Option<u64>,
}

/// Call only after durable dispatch. Every invocation gets fresh linear memory.
/// Module declarations, the exported ABI and actual host resources still apply.
pub fn execute(program: &str, input: &str) -> Outcome {
    let result = (|| {
        let input_length = u32::try_from(input.len()).map_err(|_| "input_abi_length")?;
        let (engine, module) = compile(program)?;
        let mut store = Store::new(&engine, ());
        (|| {
            let linker = Linker::<()>::new(&engine);
            let instance = linker
                .instantiate_and_start(&mut store, &module)
                .map_err(|_| "instantiation_failed")?;
            let memory = instance
                .get_memory(&store, "memory")
                .ok_or("memory_export_missing")?;
            let run = instance
                .get_typed_func::<(i32, i32), i64>(&store, "run")
                .map_err(|_| "run_signature")?;
            memory
                .write(&mut store, 0, input.as_bytes())
                .map_err(|_| "input_memory")?;
            let packed = run
                .call(&mut store, (0, input_length as i32))
                .map_err(|_| "execution_trapped")? as u64;
            let offset = (packed >> 32) as usize;
            let length = (packed & 0xffff_ffff) as usize;
            let end = offset.checked_add(length).ok_or("output_memory")?;
            let bytes = memory.data(&store).get(offset..end).ok_or("output_memory")?;
            let mut output = Vec::new();
            output.try_reserve_exact(bytes.len()).map_err(|_| "output_allocation")?;
            output.extend_from_slice(bytes);
            String::from_utf8(output).map_err(|_| "output_utf8")
        })()
    })();
    match result {
        Ok(output) => Outcome {
            succeeded: true,
            output: Some(output),
            failure: None,
            fuel_consumed: None,
        },
        Err(failure) => Outcome {
            succeeded: false,
            output: None,
            failure: Some(failure.into()),
            fuel_consumed: None,
        },
    }
}
