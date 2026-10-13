#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--public-build-info") {
        println!("{}", serde_json::json!({"schema":"nova.public.beta.build.v1","flavor":"fast-only","fullMode":false,"publicBeta":true,"gitSha":env!("LBA_BACKEND_GIT_SHA")}));
        return ;
    }

    litematica_core::native_viewer::install_viewer_shutdown_panic_hook();
    let result = (|| -> anyhow::Result<()> {
        let provenance = litematica_core::backend_provenance::collect_backend_provenance()?;
        litematica_core::backend_provenance::emit_collected_backend_provenance(&provenance)?;
        litematica_core::native_viewer::run(&provenance)
    })();
    match result {
        Ok(()) => {
            litematica_core::native_viewer::finish_viewer_main(
                "main_return_ok",
                0,
                "Viewer main returned successfully",
            );
        }
        Err(error) => {
            let detail = format!("{error:#}");
            litematica_core::native_viewer::finish_viewer_main("main_error", 1, &detail);
            eprintln!("Error: {detail}");
            std::process::exit(1);
        }
    }
}
