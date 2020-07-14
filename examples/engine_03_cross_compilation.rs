//! Defining an engine in Wasmer is one of the fundamental step.
//!
//! As a reminder, an engine applies roughly 2 steps:
//!
//!   1. It compiles the Wasm module bytes to executable code, through
//!      the intervention of a compiler,
//!   2. It stores the executable code somewhere.
//!
//! This example focuses on the first step: the compiler. It
//! illustrates how the abstraction over the compiler is so powerful
//! that it is possible to cross-compile a Wasm module.
//!
//! Ready?

use std::str::FromStr;
use std::sync::Arc;
use wasmer::{wat2wasm, Module, RuntimeError, Store};
use wasmer_compiler::{CpuFeature, Target, Triple};
use wasmer_compiler_cranelift::Cranelift;
use wasmer_engine_native::Native;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        r#"
(module
  (type $sum_t (func (param i32 i32) (result i32)))
  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)
    local.get $x
    local.get $y
    i32.add)
  (export "sum" (func $sum_f)))
"#
        .as_bytes(),
    )?;

    // Define a compiler configuration.
    //
    // In this situation, the compiler is
    // `wasmer_compiler_cranelift`. The compiler is responsible to
    // compile the Wasm module into executable code.
    let mut compiler_config = Cranelift::default();

    // Here we go.
    //
    // Let's define the target “triple”. Historically, such things had
    // three fields, though additional fields have been added over
    // time.
    let triple = Triple::from_str("x86_64-linux-musl")
        .map_err(|error| RuntimeError::new(error.to_string()))?;

    // Here we go again.
    //
    // Let's define a CPU feature.
    let mut cpu_feature = CpuFeature::set();
    cpu_feature.insert(CpuFeature::from_str("sse2")?);

    // Here we go finally.
    //
    // Let's build the target.
    let target = Target::new(triple, cpu_feature);

    // Define the engine that will drive everything.
    //
    // In this case, the engine is `wasmer_engine_native` which means
    // that a native object is going to be generated.
    //
    // That's where we specify the target for the compiler.
    let engine = Arc::new(
        // Use the native engine.
        Native::new(&mut compiler_config)
            // Here we go.
            //
            // Pass the target to the engine! The engine will share
            // this information with the compiler.
            .target(target)
            // Get the engine.
            .engine(),
    );

    // Create a store, that holds the engine.
    let store = Store::new(&*engine);

    // Let's compile the Wasm module.
    let _module = Module::new(&store, wasm_bytes)?;

    // Congrats, the Wasm module is cross-compiled!
    //
    // What to do with that? It is possible to use an engine (probably
    // a headless engine) to execute the cross-compiled Wasm module an
    // the targeted platform.

    Ok(())
}

#[test]
fn test_cross_compilation() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
