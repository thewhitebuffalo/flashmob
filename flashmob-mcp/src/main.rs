//! MCP server for one Flashmob study.
//!
//! Stdout is the protocol stream. This process does not write logs there.

mod session;

use std::sync::{Arc, Mutex};

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, ListResourcesResult, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use session::{LibraryApply, State, LABEL_NOTE};

#[derive(Clone)]
struct Flashmob {
    state: Arc<Mutex<State>>,
}

impl Flashmob {
    fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::new())),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn ok(value: Value) -> CallToolResult {
    match serde_json::to_string_pretty(&value) {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(err) => CallToolResult::error(vec![ContentBlock::text(err.to_string())]),
    }
}

fn fail(err: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(err)])
}

fn tool_result(result: Result<Value, String>) -> CallToolResult {
    match result {
        Ok(value) => ok(value),
        Err(err) => fail(err),
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenArgs {
    /// Path of a Flashmob project JSON. A path inside the Flashmob repository is refused.
    path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SaveArgs {
    /// Destination JSON. Omit to write the opened path.
    #[serde(default)]
    path: Option<String>,
    /// Replace an existing file. A path that does not exist yet is created either way.
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ApplyArgs {
    /// Exec commands, each with an `op` tag. Read flashmob://schema for the legal ops. The project is not returned.
    ops: Vec<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RunArgs {
    /// Study names: loadflow, fault, arcflash, coordination. Empty runs all of them.
    #[serde(default)]
    studies: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SldArgs {
    /// Replace an existing SVG next to the project.
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TccArgs {
    /// Voltage the curve currents are referred to, in kV.
    #[serde(default)]
    ref_kv: Option<f64>,
    /// Device ids to draw. Empty draws every device.
    #[serde(default)]
    devices: Vec<String>,
    /// Replace existing SVG and CSV files next to the project.
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchArgs {
    /// Words matched against manufacturer, category, type, style, catalog, and reference.
    query: String,
    /// Maximum hits. Values above 20 are capped at 20.
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ApplyLibraryArgs {
    /// Library device id.
    device_id: u32,
    /// Id of the new project device.
    id: String,
    /// Bus the device protects.
    bus: String,
    #[serde(default)]
    name: Option<String>,
    /// Set only when the governing band is already total clearing. Left false otherwise.
    #[serde(default)]
    fuse_total_clearing: bool,
    #[serde(default)]
    protected_branch: Option<String>,
    #[serde(default)]
    rating_a: Option<f64>,
    #[serde(default)]
    current_base_a: Option<f64>,
    #[serde(default)]
    voltage_v: Option<f64>,
    #[serde(default)]
    function_name: Option<String>,
    #[serde(default)]
    dial: Option<f64>,
    #[serde(default)]
    ct_ratio: Option<f64>,
    #[serde(default)]
    tap_a: Option<f64>,
    #[serde(default)]
    sensor_a: Option<f64>,
    #[serde(default)]
    lt_pickup_function: Option<String>,
    #[serde(default)]
    lt_pickup_label: Option<String>,
    #[serde(default)]
    lt_delay_function: Option<String>,
    #[serde(default)]
    lt_delay_label: Option<String>,
    #[serde(default)]
    lt_delay_multiple: Option<f64>,
    #[serde(default)]
    st_pickup_function: Option<String>,
    #[serde(default)]
    st_pickup_label: Option<String>,
    #[serde(default)]
    st_delay_function: Option<String>,
    #[serde(default)]
    st_delay_label: Option<String>,
    #[serde(default)]
    inst_function: Option<String>,
    #[serde(default)]
    inst_label: Option<String>,
    #[serde(default)]
    inst_s: Option<f64>,
    #[serde(default)]
    pickup_base: Option<String>,
}

impl From<ApplyLibraryArgs> for LibraryApply {
    fn from(args: ApplyLibraryArgs) -> Self {
        Self {
            device_id: args.device_id,
            id: args.id,
            bus: args.bus,
            name: args.name,
            fuse_total_clearing: args.fuse_total_clearing,
            protected_branch: args.protected_branch,
            rating_a: args.rating_a,
            current_base_a: args.current_base_a,
            voltage_v: args.voltage_v,
            function_name: args.function_name,
            dial: args.dial,
            ct_ratio: args.ct_ratio,
            tap_a: args.tap_a,
            sensor_a: args.sensor_a,
            lt_pickup_function: args.lt_pickup_function,
            lt_pickup_label: args.lt_pickup_label,
            lt_delay_function: args.lt_delay_function,
            lt_delay_label: args.lt_delay_label,
            lt_delay_multiple: args.lt_delay_multiple,
            st_pickup_function: args.st_pickup_function,
            st_pickup_label: args.st_pickup_label,
            st_delay_function: args.st_delay_function,
            st_delay_label: args.st_delay_label,
            inst_function: args.inst_function,
            inst_label: args.inst_label,
            inst_s: args.inst_s,
            pickup_base: args.pickup_base,
        }
    }
}

#[tool_router]
impl Flashmob {
    #[tool(
        description = "Load a Flashmob project JSON into memory. A path inside the Flashmob repository is refused."
    )]
    fn open(&self, Parameters(args): Parameters<OpenArgs>) -> CallToolResult {
        tool_result(self.lock().open(&args.path))
    }

    #[tool(
        description = "Write the in-memory project. The path must be the opened file's directory, ~/Documents, or /tmp. An existing file is replaced only when overwrite is true. A path inside the Flashmob repository is refused."
    )]
    fn save(&self, Parameters(args): Parameters<SaveArgs>) -> CallToolResult {
        tool_result(self.lock().save(args.path.as_deref(), args.overwrite))
    }

    #[tool(description = "Counts, ids, names, and voltages for the open project.")]
    fn summary(&self) -> CallToolResult {
        tool_result(self.lock().summary())
    }

    #[tool(
        description = "Entered connectivity of the open project: buses, branches, and what is attached to them. No study is run."
    )]
    fn topology(&self) -> CallToolResult {
        tool_result(self.lock().topology())
    }

    #[tool(
        description = "Apply a list of flashmob exec ops to the open project. Each op uses the same tagged command as flashmob exec, so the ops cannot drift from exec::Command. Read flashmob://schema for the legal ops. Returns ok or error per op, validation errors, and the ids touched. Does not return the project. export_package is refused."
    )]
    fn apply(&self, Parameters(args): Parameters<ApplyArgs>) -> CallToolResult {
        tool_result(self.lock().apply(&args.ops))
    }

    #[tool(
        description = "Run studies on the open project. Returns one row per bus and one row per arc-flash equipment item: fault current, incident energy, boundary, governing case, assumed or capped flags, warnings, and complete. Warnings, complete: false, an unresolved clearing path, or an assumption means the number is not label-ready. Feeder length and cable size must come from the model."
    )]
    fn run(&self, Parameters(args): Parameters<RunArgs>) -> CallToolResult {
        tool_result(self.lock().run(&args.studies))
    }

    #[tool(
        description = "Write an SVG one-line next to the open project and return its path and a short caption. Run a study first. Warnings, complete: false, an unresolved clearing path, or an assumption means the number is not label-ready. Feeder length and cable size must come from the model."
    )]
    fn sld(&self, Parameters(args): Parameters<SldArgs>) -> CallToolResult {
        tool_result(self.lock().sld(args.overwrite))
    }

    #[tool(
        description = "Write a time-current SVG and CSV next to the open project and return their paths and a short caption. Warnings, complete: false, an unresolved clearing path, or an assumption means the number is not label-ready. Feeder length and cable size must come from the model."
    )]
    fn tcc(&self, Parameters(args): Parameters<TccArgs>) -> CallToolResult {
        tool_result(self.lock().tcc(args.ref_kv, &args.devices, args.overwrite))
    }

    #[tool(
        description = "Search the device library named by FLASHMOB_LIBRARY. Returns at most 20 hits."
    )]
    fn library_search(&self, Parameters(args): Parameters<SearchArgs>) -> CallToolResult {
        tool_result(self.lock().library_search(&args.query, args.limit))
    }

    #[tool(
        description = "Copy one device curve from the FLASHMOB_LIBRARY file onto the open project. The curve is a snapshot. fuse_total_clearing stays off unless this call sets it. The result reports the assumed-band note."
    )]
    fn apply_library(&self, Parameters(args): Parameters<ApplyLibraryArgs>) -> CallToolResult {
        tool_result(self.lock().apply_library(args.into()))
    }
}

#[tool_handler]
impl ServerHandler for Flashmob {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new(
            "flashmob-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(LABEL_NOTE)
    }

    async fn list_resources(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(vec![Resource::new(
            "flashmob://schema",
            "schema",
        )]))
    }

    async fn read_resource(
        &self,
        request: rmcp::model::ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        if request.uri.as_str() != "flashmob://schema" {
            return Err(McpError::resource_not_found(
                "resource_not_found",
                Some(serde_json::json!({ "uri": request.uri })),
            ));
        }
        let schema = serde_json::to_string_pretty(&flashmob::exec::schema())
            .unwrap_or_else(|err| err.to_string());
        Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
            vec![ResourceContents::text(schema, &request.uri)],
        )))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = Flashmob::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
