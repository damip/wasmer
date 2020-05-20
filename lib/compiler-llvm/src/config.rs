// Allow unused imports while developing
#![allow(unused_imports, dead_code)]

use crate::compiler::LLVMCompiler;
use inkwell::targets::{
    CodeModel, InitializationConfig, RelocMode, Target as InkwellTarget, TargetMachine,
    TargetTriple,
};
use inkwell::OptimizationLevel;
use itertools::Itertools;
use target_lexicon::Architecture;
use wasmer_compiler::{Compiler, CompilerConfig, CpuFeature, Features, Target, Triple};

/// The InkWell ModuleInfo type
pub type InkwellModule<'ctx> = inkwell::module::Module<'ctx>;

/// The InkWell MemoryBuffer type
pub type InkwellMemoryBuffer = inkwell::memory_buffer::MemoryBuffer;

/// Callbacks to
pub trait LLVMCallbacks: std::any::Any + 'static {
    fn preopt_ir_callback(&mut self, module: &InkwellModule);
    fn postopt_ir_callback(&mut self, module: &InkwellModule);
    fn obj_memory_buffer_callback(&mut self, memory_buffer: &InkwellMemoryBuffer);
}

#[derive(Clone)]
pub struct LLVMConfig {
    /// Enable NaN canonicalization.
    ///
    /// NaN canonicalization is useful when trying to run WebAssembly
    /// deterministically across different architectures.
    pub enable_nan_canonicalization: bool,

    /// Should the LLVM IR verifier be enabled.
    ///
    /// The verifier assures that the generated LLVM IR is valid.
    pub enable_verifier: bool,

    /// The optimization levels when optimizing the IR.
    pub opt_level: OptimizationLevel,

    features: Features,
    target: Target,
}

impl LLVMConfig {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new(mut features: Features, target: Target) -> Self {
        // Override the default multi-value switch
        features.multi_value = false;

        let operating_system =
            if target.triple().operating_system == wasmer_compiler::OperatingSystem::Darwin {
                // LLVM detects static relocation + darwin + 64-bit and
                // force-enables PIC because MachO doesn't support that
                // combination. They don't check whether they're targeting
                // MachO, they check whether the OS is set to Darwin.
                //
                // Since both linux and darwin use SysV ABI, this should work.
                wasmer_compiler::OperatingSystem::Linux
            } else {
                target.triple().operating_system
            };
        let triple = Triple {
            architecture: target.triple().architecture,
            vendor: target.triple().vendor.clone(),
            operating_system,
            environment: target.triple().environment,
            binary_format: target_lexicon::BinaryFormat::Elf,
        };
        let target = Target::new(triple, *target.cpu_features());
        Self {
            enable_nan_canonicalization: true,
            enable_verifier: false,
            opt_level: OptimizationLevel::Aggressive,
            features,
            target,
        }
    }
    fn reloc_mode(&self) -> RelocMode {
        RelocMode::Static
    }

    fn code_model(&self) -> CodeModel {
        CodeModel::Large
    }

    pub fn target_triple(&self) -> TargetTriple {
        TargetTriple::create(&self.target().triple().to_string())
    }

    /// Generates the target machine for the current target
    pub fn target_machine(&self) -> TargetMachine {
        let target = self.target();
        let triple = target.triple();
        let cpu_features = &target.cpu_features();

        match triple.architecture {
            Architecture::X86_64 => InkwellTarget::initialize_x86(&InitializationConfig {
                asm_parser: true,
                asm_printer: true,
                base: true,
                disassembler: true,
                info: true,
                machine_code: true,
            }),
            Architecture::Arm(_) => InkwellTarget::initialize_aarch64(&InitializationConfig {
                asm_parser: true,
                asm_printer: true,
                base: true,
                disassembler: true,
                info: true,
                machine_code: true,
            }),
            _ => unimplemented!("target {} not supported", triple),
        }

        // The CPU features formatted as LLVM strings
        let llvm_cpu_features = cpu_features
            .iter()
            .filter_map(|feature| match feature {
                CpuFeature::SSE2 => Some("+sse2"),
                CpuFeature::SSE3 => Some("+sse3"),
                CpuFeature::SSSE3 => Some("+ssse3"),
                CpuFeature::SSE41 => Some("+sse4.1"),
                CpuFeature::SSE42 => Some("+sse4.2"),
                CpuFeature::POPCNT => Some("+popcnt"),
                CpuFeature::AVX => Some("+avx"),
                CpuFeature::BMI1 => Some("+bmi"),
                CpuFeature::BMI2 => Some("+bmi2"),
                CpuFeature::AVX2 => Some("+avx2"),
                CpuFeature::AVX512DQ => Some("+avx512dq"),
                CpuFeature::AVX512VL => Some("+avx512vl"),
                CpuFeature::LZCNT => Some("+lzcnt"),
            })
            .join(",");

        let arch_string = triple.architecture.to_string();
        let llvm_target = InkwellTarget::from_triple(&self.target_triple()).unwrap();
        let target_machine = llvm_target
            .create_target_machine(
                &self.target_triple(),
                "generic",
                &llvm_cpu_features,
                self.opt_level.clone(),
                self.reloc_mode(),
                self.code_model(),
            )
            .unwrap();
        target_machine
    }
}

impl CompilerConfig for LLVMConfig {
    /// Gets the WebAssembly features
    fn features(&self) -> &Features {
        &self.features
    }

    fn enable_pic(&mut self) {
        unimplemented!("PIC is not yet implemented in LLVM");
    }

    /// Gets the target that we will use for compiling
    /// the WebAssembly module
    fn target(&self) -> &Target {
        &self.target
    }

    /// Transform it into the compiler
    fn compiler(&self) -> Box<dyn Compiler + Send> {
        Box::new(LLVMCompiler::new(&self))
    }
}

impl Default for LLVMConfig {
    fn default() -> LLVMConfig {
        Self::new(Default::default(), Default::default())
    }
}
