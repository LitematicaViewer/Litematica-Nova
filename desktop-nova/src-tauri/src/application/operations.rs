//! Product-semantic one-shot operations owned by the Application Host.
//!
//! The WebView never supplies executable names, command-line arguments, or
//! temporary file paths. This module translates product requests into the
//! existing core helper contract and delegates only OS process mechanics to
//! [`ProcessSupervisor`].

use crate::platform::process_supervisor::{
    ExecutableRole, LaunchIdentity, LaunchSpec, LaunchToken, OutputTarget, ProcessSupervisor,
    WaitOutcome,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

const CORE_OPERATION_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const CORE_TERMINATE_WAIT: Duration = Duration::from_secs(3);
const MAX_DIAGNOSTIC_CHARS: usize = 16 * 1024;

#[derive(Clone)]
pub(crate) struct ApplicationOperations {
    supervisor: Arc<ProcessSupervisor>,
    project_root: PathBuf,
    data_root: PathBuf,
    backend_dir: PathBuf,
    next_generation: Arc<AtomicU64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct InspectProjectInput {
    pub(crate) source_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReadLayerInput {
    pub(crate) source_path: String,
    pub(crate) y: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReadLayerContainerInput {
    pub(crate) source_path: String,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectionMarkerPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct EditProjectOriginInput {
    pub(crate) source_path: String,
    pub(crate) origin: Option<ProjectionMarkerPoint>,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LayerPaletteEntry {
    pub(crate) block_id: String,
    pub(crate) property_id: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LayerSliceMeta {
    pub(crate) chunk_size: u32,
    pub(crate) size_x: i32,
    pub(crate) size_y: i32,
    pub(crate) size_z: i32,
    pub(crate) palette: Vec<LayerPaletteEntry>,
    pub(crate) property_pool: Vec<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LayerSliceBlock {
    pub(crate) x: i32,
    pub(crate) z: i32,
    pub(crate) palette_id: usize,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LayerSliceData {
    pub(crate) y: i32,
    pub(crate) blocks: Vec<LayerSliceBlock>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct LayerContainerData {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) z: i32,
    pub(crate) block_id: String,
    pub(crate) nbt: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ProjectionMarkers {
    pub(crate) origin: Option<ProjectionMarkerPoint>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ProjectionMarkerEditSummary {
    pub(crate) input: String,
    pub(crate) output: String,
    pub(crate) backup_path: Option<String>,
    pub(crate) origin: Option<ProjectionMarkerPoint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct EditProjectMetadataInput {
    pub(crate) source_path: String,
    pub(crate) patch: Value,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProjectMutationResult {
    pub(crate) message: String,
    pub(crate) output_path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ReplaceBlocksInput {
    pub(crate) source_path: String,
    pub(crate) output_path: Option<String>,
    pub(crate) rules: Vec<Value>,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplaceBlocksResult {
    pub(crate) message: String,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProjectStatsInput {
    pub(crate) source_path: String,
    pub(crate) include_container_items: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MaterialScopeKind {
    All,
    Region,
    Layer,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct MaterialScope {
    pub(crate) kind: MaterialScopeKind,
    pub(crate) value: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct ProjectMaterialsInput {
    pub(crate) source_path: String,
    pub(crate) scope: MaterialScope,
    pub(crate) include_container_items: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct GenerateProjectInput {
    pub(crate) plan: Value,
    pub(crate) output_path: Option<String>,
    pub(crate) dry_run: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GenerateProjectResult {
    pub(crate) summary: Value,
    pub(crate) analysis: Option<Value>,
    pub(crate) output_path: Option<String>,
    pub(crate) diagnostics: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeComponentHealth {
    pub(crate) available: bool,
    pub(crate) message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeHealth {
    pub(crate) ok: bool,
    pub(crate) core: RuntimeComponentHealth,
    pub(crate) viewer: RuntimeComponentHealth,
}

#[derive(Debug, Serialize)]
pub(crate) struct EmbeddedPreview {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data_url: String,
}

#[derive(Debug, Deserialize)]
struct CoreProjectionPreviewResult {
    found: bool,
    width: Option<u32>,
    height: Option<u32>,
    output_path: Option<PathBuf>,
}

struct CoreOutput {
    stdout: String,
}

struct OperationFiles {
    paths: Vec<PathBuf>,
}

impl OperationFiles {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn track(&mut self, path: PathBuf) -> PathBuf {
        self.paths.push(path.clone());
        path
    }
}

impl Drop for OperationFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

impl ApplicationOperations {
    pub(crate) fn new(
        supervisor: Arc<ProcessSupervisor>,
        project_root: PathBuf,
        data_root: PathBuf,
        backend_dir: PathBuf,
    ) -> Result<Self, String> {
        if !project_root.is_absolute() || !project_root.is_dir() {
            return Err(
                "Application project root must be an existing absolute directory".to_owned(),
            );
        }
        if !backend_dir.is_absolute() || !backend_dir.is_dir() {
            return Err(
                "Application backend directory must be an existing absolute directory".to_owned(),
            );
        }
        let operations_root = data_root.join("application").join("operations");
        fs::create_dir_all(&operations_root).map_err(|error| {
            format!(
                "create Application operation directory {}: {error}",
                operations_root.display()
            )
        })?;
        Ok(Self {
            supervisor,
            project_root,
            data_root,
            backend_dir,
            next_generation: Arc::new(AtomicU64::new(1)),
        })
    }

    pub(crate) fn inspect_project(&self, input: InspectProjectInput) -> Result<Value, String> {
        let source = self.resolve_path(&input.source_path);
        self.run_core_json(vec!["analyze".to_owned(), path_text(&source)])
    }

    pub(crate) fn read_layer_meta(&self, input: InspectProjectInput) -> Result<LayerSliceMeta, String> {
        let source = self.resolve_path(&input.source_path);
        let result = self.run_core_json(vec!["visual-meta".to_owned(), path_text(&source)])?;
        decode_product(result.get("visual").cloned().unwrap_or(Value::Null))
    }

    pub(crate) fn read_layer(&self, input: ReadLayerInput) -> Result<LayerSliceData, String> {
        let source = self.resolve_path(&input.source_path);
        decode_product(self.run_core_json(layer_args(&source, input.y)?)?)
    }

    pub(crate) fn read_layer_container(
        &self,
        input: ReadLayerContainerInput,
    ) -> Result<Option<LayerContainerData>, String> {
        let source = self.resolve_path(&input.source_path);
        decode_product(self.run_core_json(layer_container_args(&source, input.x, input.y, input.z)?)?)
    }

    pub(crate) fn read_project_markers(&self, input: InspectProjectInput) -> Result<ProjectionMarkers, String> {
        let source = self.resolve_path(&input.source_path);
        decode_product(self.run_core_json(vec!["projection-markers".to_owned(), path_text(&source)])?)
    }

    pub(crate) fn edit_project_origin(
        &self,
        input: EditProjectOriginInput,
    ) -> Result<ProjectionMarkerEditSummary, String> {
        let source = self.resolve_path(&input.source_path);
        if input.output_path.as_ref().is_some_and(|path| path.trim().is_empty()) {
            return Err("The projection output path must not be empty".to_owned());
        }
        let output = input.output_path.as_deref().map(|path| self.resolve_path(path));
        let (source, output) = protected_origin_paths(&source, output.as_deref())?;
        decode_product(self.run_core_json(origin_args(&source, input.origin, output.as_deref()))?)
    }

    pub(crate) fn edit_project_metadata(
        &self,
        input: EditProjectMetadataInput,
    ) -> Result<ProjectMutationResult, String> {
        let source = self.resolve_path(&input.source_path);
        let output = input
            .output_path
            .as_deref()
            .map(|path| self.resolve_path(path))
            .unwrap_or_else(|| source.clone());
        let (generation, prefix) = self.next_operation("metadata");
        let mut files = OperationFiles::new();
        let patch_path = files.track(self.operation_path(&prefix, "patch.json"));
        write_json(&patch_path, &input.patch)?;
        let mut args = vec![
            "edit-metadata".to_owned(),
            "--input".to_owned(),
            path_text(&source),
            "--patch".to_owned(),
            path_text(&patch_path),
        ];
        if output != source {
            args.extend(["--output".to_owned(), path_text(&output)]);
        }
        let result = self.run_core(generation, &prefix, args, &mut files)?;
        Ok(ProjectMutationResult {
            message: semantic_message(&result.stdout),
            output_path: path_text(&output),
        })
    }

    pub(crate) fn replace_blocks(
        &self,
        input: ReplaceBlocksInput,
    ) -> Result<ReplaceBlocksResult, String> {
        if input.rules.is_empty() {
            return Err("At least one replacement rule is required".to_owned());
        }
        if input.dry_run && input.output_path.is_some() {
            return Err("A dry-run replacement may not specify an output path".to_owned());
        }
        if !input.dry_run && input.output_path.is_none() {
            return Err("A writing replacement requires an output path".to_owned());
        }
        let source = self.resolve_path(&input.source_path);
        let output = input
            .output_path
            .as_deref()
            .map(|path| self.resolve_path(path));
        let (generation, prefix) = self.next_operation("replace");
        let mut files = OperationFiles::new();
        let rules_path = files.track(self.operation_path(&prefix, "rules.json"));
        write_json(&rules_path, &json!({ "rules": input.rules }))?;
        let args = replacement_args(&source, &rules_path, output.as_deref(), input.dry_run);
        let result = self.run_core(generation, &prefix, args, &mut files)?;
        Ok(ReplaceBlocksResult {
            message: semantic_message(&result.stdout),
            output_path: output.as_deref().map(path_text),
        })
    }

    pub(crate) fn project_stats(&self, input: ProjectStatsInput) -> Result<Value, String> {
        let source = self.resolve_path(&input.source_path);
        self.run_core_json(stats_args(&source, input.include_container_items))
    }

    pub(crate) fn project_materials(&self, input: ProjectMaterialsInput) -> Result<Value, String> {
        let source = self.resolve_path(&input.source_path);
        let result = self.run_core_json(materials_args(
            &source,
            &input.scope,
            input.include_container_items,
        )?)?;
        Ok(json!({
            "material_items": result
                .get("material_items")
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new()))
        }))
    }

    pub(crate) fn generate_project(
        &self,
        input: GenerateProjectInput,
    ) -> Result<GenerateProjectResult, String> {
        if input.dry_run && input.output_path.is_some() {
            return Err("A dry-run generation may not specify an output path".to_owned());
        }
        if !input.dry_run && input.output_path.is_none() {
            return Err("A writing generation requires an output path".to_owned());
        }
        let output = input
            .output_path
            .as_deref()
            .map(|path| self.resolve_path(path));
        let (generation, prefix) = self.next_operation("generate");
        let mut files = OperationFiles::new();
        let plan_path = files.track(self.operation_path(&prefix, "plan.json"));
        write_json(&plan_path, &input.plan)?;
        let core = self.run_core(
            generation,
            &prefix,
            generate_args(&plan_path, output.as_deref(), input.dry_run),
            &mut files,
        )?;
        let mut summary = parse_core_json(&core.stdout)?;
        if let Some(object) = summary.as_object_mut() {
            object.remove("plan_path");
        }
        let (analysis, analysis_warning) = match output.as_ref() {
            Some(path) => match self.run_core_json(vec!["analyze".to_owned(), path_text(path)]) {
                Ok(analysis) => (Some(analysis), None),
                Err(error) => (
                    None,
                    Some(format!(
                        "Generated project was written successfully, but follow-up analysis failed: {}",
                        bounded_text(&error)
                    )),
                ),
            },
            None => (None, None),
        };
        let mut diagnostics = serde_json::to_string_pretty(&summary)
            .unwrap_or_else(|_| "Generation completed".to_owned());
        if let Some(warning) = analysis_warning {
            diagnostics.push_str("\n\n");
            diagnostics.push_str(&warning);
        }
        Ok(GenerateProjectResult {
            summary,
            analysis,
            output_path: output.as_deref().map(path_text),
            diagnostics,
        })
    }

    pub(crate) fn runtime_health(&self) -> RuntimeHealth {
        let viewer_available = cfg!(windows) && self
            .backend_dir
            .join(format!("litematica_native_viewer{}", std::env::consts::EXE_SUFFIX))
            .is_file();
        let core = match self.run_core_text(vec!["--help".to_owned()]) {
            Ok(help) if !help.trim().is_empty() => RuntimeComponentHealth {
                available: true,
                message: "Core operations are available".to_owned(),
            },
            Ok(_) => RuntimeComponentHealth {
                available: false,
                message: "Core helper returned an empty capability response".to_owned(),
            },
            Err(error) => RuntimeComponentHealth {
                available: false,
                message: bounded_text(&error),
            },
        };
        let viewer = RuntimeComponentHealth {
            available: viewer_available,
            message: if viewer_available {
                "Native preview is available".to_owned()
            } else {
                "Native preview component is unavailable".to_owned()
            },
        };
        RuntimeHealth {
            ok: core.available && viewer.available,
            core,
            viewer,
        }
    }

    pub(crate) fn read_embedded_preview(
        &self,
        input: InspectProjectInput,
    ) -> Result<Option<EmbeddedPreview>, String> {
        let source = self.resolve_path(&input.source_path);
        if !source.is_file() {
            return Err(format!(
                "Projection preview source is missing: {}",
                source.display()
            ));
        }
        let (generation, prefix) = self.next_operation("embedded-preview");
        let mut files = OperationFiles::new();
        let output_path = files.track(self.operation_path(&prefix, "preview.png"));
        let core = self.run_core(
            generation,
            &prefix,
            vec![
                "projection-preview".to_owned(),
                "--input".to_owned(),
                path_text(&source),
                "--output".to_owned(),
                path_text(&output_path),
                "--json".to_owned(),
            ],
            &mut files,
        )?;
        let result: CoreProjectionPreviewResult = serde_json::from_str(core.stdout.trim())
            .map_err(|error| format!("decode embedded preview result: {error}"))?;
        if !result.found {
            if result.width.is_some() || result.height.is_some() || result.output_path.is_some() {
                return Err("Embedded preview returned metadata for found=false".to_owned());
            }
            if output_path.exists() {
                return Err("Embedded preview published an image for found=false".to_owned());
            }
            return Ok(None);
        }
        let width = result
            .width
            .filter(|value| *value > 0)
            .ok_or_else(|| "Embedded preview omitted a positive width".to_owned())?;
        let height = result
            .height
            .filter(|value| *value > 0)
            .ok_or_else(|| "Embedded preview omitted a positive height".to_owned())?;
        let reported = result
            .output_path
            .ok_or_else(|| "Embedded preview omitted its output path".to_owned())?;
        let expected = fs::canonicalize(&output_path).map_err(|error| {
            format!(
                "resolve embedded preview {}: {error}",
                output_path.display()
            )
        })?;
        let reported = fs::canonicalize(&reported).map_err(|error| {
            format!(
                "resolve reported embedded preview {}: {error}",
                reported.display()
            )
        })?;
        if expected != reported {
            return Err("Embedded preview reported an unexpected output path".to_owned());
        }
        let bytes = fs::read(&expected)
            .map_err(|error| format!("read embedded preview {}: {error}", expected.display()))?;
        validate_png_dimensions(&bytes, width, height)?;
        Ok(Some(EmbeddedPreview {
            width,
            height,
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ),
        }))
    }

    fn run_core_json(&self, args: Vec<String>) -> Result<Value, String> {
        let output = self.run_core_text(args)?;
        parse_core_json(&output)
    }

    fn run_core_text(&self, args: Vec<String>) -> Result<String, String> {
        let (generation, prefix) = self.next_operation("core");
        let mut files = OperationFiles::new();
        self.run_core(generation, &prefix, args, &mut files)
            .map(|output| output.stdout)
    }

    fn run_core(
        &self,
        generation: u64,
        prefix: &str,
        args: Vec<String>,
        files: &mut OperationFiles,
    ) -> Result<CoreOutput, String> {
        let stdout_path = files.track(self.operation_path(prefix, "stdout.log"));
        let stderr_path = files.track(self.operation_path(prefix, "stderr.log"));
        let identity = LaunchIdentity {
            owner_epoch: self.supervisor.owner_epoch(),
            generation,
            launch_token: LaunchToken::new(format!("application-core-{prefix}"))
                .map_err(|error| format!("create core operation launch token: {error}"))?,
        };
        let mut launch = LaunchSpec::new(ExecutableRole::CoreHelper, identity);
        launch.args = args.into_iter().map(Into::into).collect();
        launch.environment = vec![(
            "LBA_DATA_ROOT".into(),
            self.data_root.as_os_str().to_owned(),
        )];
        launch.current_dir = Some(self.backend_dir.clone());
        launch.stdout = OutputTarget::File {
            path: stdout_path.clone(),
            append: false,
        };
        launch.stderr = OutputTarget::File {
            path: stderr_path.clone(),
            append: false,
        };
        let child = self
            .supervisor
            .spawn(launch)
            .map_err(|error| format!("start core operation: {error}"))?
            .child;
        let exit = match self
            .supervisor
            .wait_for_exit(&child, CORE_OPERATION_TIMEOUT)
        {
            WaitOutcome::Exited(exit) => exit,
            WaitOutcome::TimedOut => {
                let _ = self.supervisor.terminate(&child, 1);
                let _ = self.supervisor.wait_for_exit(&child, CORE_TERMINATE_WAIT);
                return Err("Core operation exceeded its bounded execution time".to_owned());
            }
            WaitOutcome::Stale => {
                return Err("Core operation lost its supervised process lease".to_owned())
            }
        };
        let stdout = read_output(&stdout_path)?;
        let stderr = read_output(&stderr_path)?;
        if exit.wait_error.is_some() {
            return Err(format!(
                "Core operation could not be reaped: {}",
                exit.wait_error.unwrap_or_default()
            ));
        }
        if exit.exit_code != Some(0) {
            return Err(format!(
                "Core operation failed: {}",
                bounded_text(if stderr.trim().is_empty() {
                    &stdout
                } else {
                    &stderr
                })
            ));
        }
        Ok(CoreOutput { stdout })
    }

    fn resolve_path(&self, value: &str) -> PathBuf {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            self.project_root.join(path)
        }
    }

    fn next_operation(&self, kind: &str) -> (u64, String) {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let prefix = format!("{kind}-{}-{generation}", std::process::id());
        (generation, prefix)
    }

    fn operation_path(&self, prefix: &str, suffix: &str) -> PathBuf {
        self.data_root
            .join("application")
            .join("operations")
            .join(format!("{prefix}-{suffix}"))
    }
}

fn layer_args(source: &Path, y: i32) -> Result<Vec<String>, String> {
    if y < 0 {
        return Err("Layer coordinates must be non-negative".to_owned());
    }
    Ok(vec!["visualize-layer".to_owned(), path_text(source), format!("--y={y}")])
}

fn layer_container_args(source: &Path, x: i32, y: i32, z: i32) -> Result<Vec<String>, String> {
    if x < 0 || y < 0 || z < 0 {
        return Err("Layer coordinates must be non-negative".to_owned());
    }
    Ok(vec![
        "read-layer-container".to_owned(), path_text(source),
        format!("--x={x}"), format!("--y={y}"), format!("--z={z}"),
    ])
}

fn origin_args(source: &Path, origin: Option<ProjectionMarkerPoint>, output: Option<&Path>) -> Vec<String> {
    let mut args = vec![
        if origin.is_some() { "set-origin" } else { "remove-origin" }.to_owned(),
        path_text(source),
    ];
    if let Some(point) = origin {
        args.extend([format!("--x={}", point.x), format!("--y={}", point.y), format!("--z={}", point.z)]);
    }
    if let Some(output) = output {
        args.push(format!("--output={}", path_text(output)));
    }
    args
}

/// Core owns in-place backup/restore; a separate output must be a new file.
/// Core also uses exclusive creation so this preflight is not the only overwrite guard.
fn protected_origin_paths(source: &Path, output: Option<&Path>) -> Result<(PathBuf, Option<PathBuf>), String> {
    let source = fs::canonicalize(source).map_err(|error| format!("Resolve projection source: {error}"))?;
    if !source.is_file() {
        return Err("Projection source must be a file".to_owned());
    }
    let Some(output) = output else { return Ok((source, None)); };
    if output.exists() {
        return Err("The Save As destination already exists; choose a new file".to_owned());
    }
    let parent = output.parent().filter(|parent| parent.is_dir())
        .ok_or_else(|| "Projection output directory does not exist".to_owned())?;
    let name = output.file_name().ok_or_else(|| "Projection output requires a file name".to_owned())?;
    let output = fs::canonicalize(parent).map_err(|error| format!("Resolve projection output directory: {error}"))?.join(name);
    Ok((source, Some(output)))
}

fn decode_product<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Core operation returned invalid product data: {error}"))
}

fn stats_args(source: &Path, include_container_items: bool) -> Vec<String> {
    let mut args = vec![
        "stats".to_owned(),
        "--input".to_owned(),
        path_text(source),
        "--json".to_owned(),
    ];
    if include_container_items {
        args.push("--include-container-items".to_owned());
    }
    args
}

fn materials_args(
    source: &Path,
    scope: &MaterialScope,
    include_container_items: bool,
) -> Result<Vec<String>, String> {
    let mut args = vec![
        "materials".to_owned(),
        "--input".to_owned(),
        path_text(source),
    ];
    match scope.kind {
        MaterialScopeKind::All => args.extend(["--scope".to_owned(), "all".to_owned()]),
        MaterialScopeKind::Region | MaterialScopeKind::Layer => {
            let value = scope
                .value
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "The selected material scope requires a value".to_owned())?;
            let flag = match scope.kind {
                MaterialScopeKind::Region => "--region",
                MaterialScopeKind::Layer => "--layer",
                MaterialScopeKind::All => unreachable!(),
            };
            args.extend([flag.to_owned(), value.to_owned()]);
        }
    }
    args.push("--json".to_owned());
    if include_container_items {
        args.push("--include-container-items".to_owned());
    }
    Ok(args)
}

fn replacement_args(
    source: &Path,
    rules: &Path,
    output: Option<&Path>,
    dry_run: bool,
) -> Vec<String> {
    let mut args = vec![
        "replace-blocks".to_owned(),
        "--input".to_owned(),
        path_text(source),
        "--rules".to_owned(),
        path_text(rules),
    ];
    if let Some(output) = output {
        args.extend(["--output".to_owned(), path_text(output)]);
    }
    if dry_run {
        args.push("--dry-run".to_owned());
    }
    args
}

fn generate_args(plan: &Path, output: Option<&Path>, dry_run: bool) -> Vec<String> {
    let mut args = vec!["generate".to_owned(), "--plan".to_owned(), path_text(plan)];
    if let Some(output) = output {
        args.extend(["--output".to_owned(), path_text(output)]);
    }
    if dry_run {
        args.push("--dry-run".to_owned());
    }
    args
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("encode Application operation input: {error}"))?;
    fs::write(path, bytes).map_err(|error| {
        format!(
            "write Application operation input {}: {error}",
            path.display()
        )
    })
}

fn read_output(path: &Path) -> Result<String, String> {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|error| {
            format!(
                "read Application operation output {}: {error}",
                path.display()
            )
        })
}

fn parse_core_json(output: &str) -> Result<Value, String> {
    serde_json::from_str(output.trim())
        .map_err(|error| format!("Core operation returned invalid product data: {error}"))
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn semantic_message(stdout: &str) -> String {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        "Operation completed".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn bounded_text(value: &str) -> String {
    let mut chars = value.chars();
    let bounded = chars
        .by_ref()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{bounded}\n[diagnostic truncated]")
    } else {
        bounded
    }
}

fn validate_png_dimensions(bytes: &[u8], width: u32, height: u32) -> Result<(), String> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return Err("Embedded preview did not produce a valid PNG header".to_owned());
    }
    let actual_width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let actual_height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if actual_width != width || actual_height != height {
        return Err(format!(
            "Embedded preview dimensions differ from its product metadata: {actual_width}x{actual_height} != {width}x{height}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flake_requests_reject_process_and_cache_fields() {
        assert!(serde_json::from_value::<InspectProjectInput>(json!({
            "sourcePath": "scene.litematic", "cachePath": "cache.bin"
        })).is_err());
        assert!(serde_json::from_value::<ReadLayerInput>(json!({
            "sourcePath": "scene.litematic", "y": 1, "args": ["--help"]
        })).is_err());
        assert!(serde_json::from_value::<ReadLayerContainerInput>(json!({
            "sourcePath": "scene.litematic", "x": 0, "y": 1, "z": 2, "executable": "other.exe"
        })).is_err());
        assert!(serde_json::from_value::<EditProjectOriginInput>(json!({
            "sourcePath": "scene.litematic", "origin": { "x": 0, "y": 1, "z": 2, "args": [] }, "outputPath": null
        })).is_err());
        assert!(serde_json::from_value::<EditProjectOriginInput>(json!({
            "sourcePath": "scene.litematic", "origin": null, "outputPath": null, "args": []
        })).is_err());
    }

    #[test]
    fn flake_coordinates_are_bounded_integers() {
        for y in [json!(0.5), json!(2147483648_i64), json!("--help")] {
            assert!(serde_json::from_value::<ReadLayerInput>(json!({
                "sourcePath": "scene.litematic", "y": y
            })).is_err());
        }
        assert!(layer_args(Path::new("scene.litematic"), -1).is_err());
        assert!(layer_container_args(Path::new("scene.litematic"), 0, 0, -1).is_err());
    }

    #[test]
    fn flake_arguments_use_source_not_render_cache_and_layer_coordinates() {
        let source = Path::new(r"C:\projection files\scene.litematic");
        assert_eq!(layer_args(source, 42).unwrap(), vec!["visualize-layer", r"C:\projection files\scene.litematic", "--y=42"]);
        assert_eq!(layer_container_args(source, 2, 42, 3).unwrap(), vec!["read-layer-container", r"C:\projection files\scene.litematic", "--x=2", "--y=42", "--z=3"]);
        assert_eq!(origin_args(source, None, None), vec!["remove-origin", r"C:\projection files\scene.litematic"]);
        assert_eq!(origin_args(source, Some(ProjectionMarkerPoint { x: -4, y: 5, z: 6 }), Some(Path::new(r"C:\copy.litematic"))), vec!["set-origin", r"C:\projection files\scene.litematic", "--x=-4", "--y=5", "--z=6", r"--output=C:\copy.litematic"]);
    }

    #[test]
    fn flake_output_protection_allows_in_place_and_new_files_only() {
        let root = std::env::temp_dir().join(format!("nova-origin-paths-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&root).unwrap();
        let source = root.join("source.litematic");
        let occupied = root.join("occupied.litematic");
        let new_file = root.join("new.litematic");
        fs::write(&source, b"source").unwrap();
        fs::write(&occupied, b"existing").unwrap();
        assert!(protected_origin_paths(&source, None).unwrap().1.is_none());
        assert!(protected_origin_paths(&source, Some(&source)).is_err());
        assert!(protected_origin_paths(&source, Some(&root.join(".").join("source.litematic"))).is_err());
        #[cfg(windows)]
        assert!(protected_origin_paths(&source, Some(&root.join("SOURCE.LITEMATIC"))).is_err());
        assert!(protected_origin_paths(&source, Some(&occupied)).is_err());
        assert!(protected_origin_paths(&source, Some(&root)).is_err());
        assert!(protected_origin_paths(&source, Some(&root.join("missing/new.litematic"))).is_err());
        assert!(protected_origin_paths(&source, Some(&new_file)).unwrap().1.is_some());
        assert!(!new_file.exists());
        assert_eq!(fs::read(&source).unwrap(), b"source");
        assert_eq!(fs::read(&occupied).unwrap(), b"existing");
        fs::remove_file(source).unwrap();
        fs::remove_file(occupied).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn flake_typed_responses_preserve_null_and_reject_malformed_payloads() {
        assert!(decode_product::<Option<LayerContainerData>>(Value::Null).unwrap().is_none());
        assert!(decode_product::<LayerSliceData>(json!({ "y": 3, "blocks": [] })).is_ok());
        assert!(decode_product::<LayerSliceData>(json!({ "y": 3, "blocks": null })).is_err());
        assert!(decode_product::<ProjectionMarkerEditSummary>(json!({
            "input": "a", "output": "b", "backup_path": null, "origin": { "x": -1, "y": 2, "z": 3 }
        })).is_ok());
    }

    #[test]
    fn replacement_always_uses_the_request_owned_rules_file() {
        let args = replacement_args(
            Path::new(r"C:\scene.litematic"),
            Path::new(r"C:\runtime\replace-rules.json"),
            Some(Path::new(r"C:\scene-replaced.litematic")),
            false,
        );
        assert_eq!(args[0], "replace-blocks");
        assert_eq!(args[3], "--rules");
        assert_eq!(args[4], r"C:\runtime\replace-rules.json");
        assert_eq!(args[5], "--output");
        assert!(!args.contains(&"--dry-run".to_owned()));
    }

    #[test]
    fn material_scope_is_translated_from_product_semantics() {
        let args = materials_args(
            Path::new(r"C:\scene.litematic"),
            &MaterialScope {
                kind: MaterialScopeKind::Layer,
                value: Some("42".to_owned()),
            },
            true,
        )
        .unwrap();
        assert!(args.windows(2).any(|pair| pair == ["--layer", "42"]));
        assert!(args.contains(&"--include-container-items".to_owned()));
    }

    #[test]
    fn embedded_preview_rejects_mismatched_dimensions() {
        let mut png = vec![0_u8; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&320_u32.to_be_bytes());
        png[20..24].copy_from_slice(&180_u32.to_be_bytes());
        assert!(validate_png_dimensions(&png, 320, 180).is_ok());
        assert!(validate_png_dimensions(&png, 640, 360).is_err());
    }

    #[test]
    fn generation_does_not_expose_its_temporary_plan_as_product_output() {
        let mut value = json!({ "plan_path": "C:/temp/internal.json", "dry_run": true });
        value.as_object_mut().unwrap().remove("plan_path");
        assert!(value.get("plan_path").is_none());
    }
}
