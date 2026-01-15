use pico_vm::{
    compiler::riscv::compiler::{Compiler, SourceType},
    configs::config::StarkGenericConfig,
    configs::config::Val,
    instances::{
        chiptype::riscv_chiptype::RiscvChipType,
        configs::riscv_kb_config::StarkConfig as RiscvKbSC,
        machine::riscv::RiscvMachine,
    },
    machine::{keys::HashableKey, machine::MachineBehavior},
    primitives::consts::RISCV_NUM_PVS,
};

fn main() {
    let elf_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: brevis-pico-vkey <path-to-riscv-elf>");
        std::process::exit(2);
    });

    let elf_bytes = std::fs::read(&elf_path).unwrap_or_else(|err| {
        eprintln!("Failed to read ELF {}: {}", elf_path, err);
        std::process::exit(1);
    });

    let compiler = Compiler::new(SourceType::RISCV, &elf_bytes);
    let program = compiler.compile();

    let chips = RiscvChipType::<Val<RiscvKbSC>>::all_chips();
    let machine = RiscvMachine::new(RiscvKbSC::new(), chips, RISCV_NUM_PVS);

    let (_pk, vk) = machine.setup_keys(&program);
    let vkey = vk.hash_str_via_bn254();
    let vkey = vkey.strip_prefix("0x").unwrap_or(&vkey);

    println!("{vkey}");
}
