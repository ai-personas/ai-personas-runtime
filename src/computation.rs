//! Bounded, import-free WebAssembly computation. This module supplies no host
//! capabilities. Runtime authority and durable dispatch belong to runtime::tools.
use crate::store::sha256_bytes;
use serde::{Deserialize, Serialize};
use wasmi::{Config, EnforcedLimits, Engine, Linker, Module, Store, StoreLimitsBuilder};

pub const PROFILE: &str = "compute.wasm.v1";
pub const PROGRAM_BYTES: usize = 32_768;
pub const INPUT_BYTES: usize = 8_192;
pub const OUTPUT_BYTES: usize = 8_192;
pub const FUEL: u64 = 1_000_000;
pub const MEMORY_BYTES: usize = 2 * 1024 * 1024;

/// Changes to the runner or resolved dependencies invalidate outstanding grants.
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
        .consume_fuel(true)
        .allow_start_fn(false)
        .wasm_multi_memory(false)
        .set_max_recursion_depth(128)
        .set_min_stack_height(256)
        .set_max_stack_height(4096)
        .set_max_cached_stacks(0)
        .enforced_limits(EnforcedLimits::strict());
    Engine::new(&config)
}

/// Validation never instantiates or executes the program. No imports, including
/// WASI, filesystem, network, clocks, randomness or model calls, are linked.
pub fn validate(program: &str) -> Result<(), &'static str> {
    compile(program).map(|_| ())
}
fn compile(program: &str) -> Result<(Engine, Module), &'static str> {
    if program.is_empty() || program.len() > PROGRAM_BYTES {
        return Err("program_size");
    }
    let engine = engine();
    let module = Module::new(&engine, program.as_bytes()).map_err(|_| "invalid_module")?;
    if module.imports().next().is_some() {
        return Err("imports_forbidden");
    }
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
    pub fuel_consumed: u64,
}

/// Call only after durable dispatch and a prepaid result slot. Every invocation
/// gets fresh linear memory. Fuel, linear memory, table and stack limits apply;
/// these are not a bound on the host process's RSS or compilation wall time.
pub fn execute(program: &str, input: &str) -> Outcome {
    let mut fuel_consumed = 0;
    let result = (|| {
        if input.len() > INPUT_BYTES {
            return Err("input_size");
        }
        let (engine, module) = compile(program)?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(MEMORY_BYTES)
            .memories(1)
            .tables(1)
            .table_elements(4096)
            .instances(1)
            .trap_on_grow_failure(true)
            .build();
        let mut store = Store::new(&engine, limits);
        store.limiter(|limits| limits);
        store.set_fuel(FUEL).map_err(|_| "fuel_configuration")?;
        let result = (|| {
            let linker = Linker::<wasmi::StoreLimits>::new(&engine);
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
                .call(&mut store, (0, input.len() as i32))
                .map_err(|_| "execution_trapped")? as u64;
            let offset = (packed >> 32) as usize;
            let length = (packed & 0xffff_ffff) as usize;
            if length > OUTPUT_BYTES {
                return Err("output_size");
            }
            let mut output = vec![0; length];
            memory
                .read(&store, offset, &mut output)
                .map_err(|_| "output_memory")?;
            String::from_utf8(output).map_err(|_| "output_utf8")
        })();
        fuel_consumed = FUEL - store.get_fuel().map_err(|_| "fuel_accounting")?;
        result
    })();
    match result {
        Ok(output) => Outcome {
            succeeded: true,
            output: Some(output),
            failure: None,
            fuel_consumed,
        },
        Err(failure) => Outcome {
            succeeded: false,
            output: None,
            failure: Some(failure.into()),
            fuel_consumed,
        },
    }
}
