#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
fn main() {
    match nova_protected_loader::run_role(nova_protected_loader::Role::BuildWorker) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("[NOVA_BUILD_WORKER] status=fatal protected_start_failed error={error:#}");
            std::process::exit(1);
        }
    }
}
