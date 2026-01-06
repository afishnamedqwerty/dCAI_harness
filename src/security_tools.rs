//! Security tool discovery and execution
//!
//! This module provides dynamic discovery and execution of security tools
//! from a tools directory. Tools can optionally have a `tool.json` metadata
//! file for richer descriptions.

use crate::error::Result;
use crate::tools::{JsonSchema, Tool, ToolContext, ToolOutput};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Category of security tool
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum SecurityCategory {
    Network,
    Process,
    Rootkit,
    Hardening,
    Filesystem,
    General,
}

impl Default for SecurityCategory {
    fn default() -> Self {
        Self::General
    }
}

impl std::fmt::Display for SecurityCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network => write!(f, "network"),
            Self::Process => write!(f, "process"),
            Self::Rootkit => write!(f, "rootkit"),
            Self::Hardening => write!(f, "hardening"),
            Self::Filesystem => write!(f, "filesystem"),
            Self::General => write!(f, "general"),
        }
    }
}

/// Metadata for a security tool (from tool.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: SecurityCategory,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub args: Vec<ToolArg>,
    #[serde(default)]
    pub requires_sudo: bool,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Argument definition for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolArg {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<String>,
}

/// A discovered security tool
#[derive(Debug, Clone)]
pub struct SecurityTool {
    /// Unique identifier (derived from filename)
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description for LLM
    pub description: String,
    /// Tool category
    pub category: SecurityCategory,
    /// Tags for filtering (e.g., "security_tools", "web_tools")
    pub tags: Vec<String>,
    /// Path to executable
    pub command_path: PathBuf,
    /// Whether sudo is required
    pub requires_sudo: bool,
    /// Timeout in seconds
    pub timeout_secs: Option<u64>,
    /// Argument definitions
    pub args: Vec<ToolArg>,
}

impl SecurityTool {
    /// Execute this tool with the given arguments
    pub fn execute(&self, args: &[String]) -> ToolOutput {
        let mut cmd = if self.requires_sudo {
            let mut c = Command::new("sudo");
            if let Some(timeout) = self.timeout_secs {
                c.arg("timeout").arg(timeout.to_string());
            }
            c.arg(&self.command_path);
            c
        } else {
            let c = if let Some(timeout) = self.timeout_secs {
                let mut tc = Command::new("timeout");
                tc.arg(timeout.to_string());
                tc.arg(&self.command_path);
                tc
            } else {
                Command::new(&self.command_path)
            };
            c
        };

        cmd.args(args);

        match cmd.output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                let content = if stdout.is_empty() && !stderr.is_empty() {
                    format!("STDERR:\n{}", stderr)
                } else if !stderr.is_empty() {
                    format!("{}\n\nSTDERR:\n{}", stdout, stderr)
                } else {
                    stdout.to_string()
                };

                if output.status.success() {
                    ToolOutput::success(content)
                } else {
                    ToolOutput::failure_with_content(
                        content,
                        format!("Tool exited with status: {}", output.status),
                    )
                }
            }
            Err(e) => ToolOutput::failure(format!("Failed to execute tool: {}", e)),
        }
    }
}

/// Registry of discovered security tools
#[derive(Debug, Clone)]
pub struct SecurityToolRegistry {
    tools_dir: PathBuf,
    tools: HashMap<String, SecurityTool>,
    /// Semaphore for controlling parallel execution (None = sequential)
    parallel_semaphore: Option<Arc<Semaphore>>,
}

impl SecurityToolRegistry {
    /// Discover all security tools from a directory
    ///
    /// Looks for:
    /// - Executable files (scripts, binaries)
    /// - Optional `tool.json` metadata files
    /// - MCP tool directories (with Cargo.toml)
    pub fn discover(tools_dir: impl AsRef<Path>) -> Self {
        let tools_dir = tools_dir.as_ref().to_path_buf();
        let mut tools = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(&tools_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Handle directories (potential MCP tools)
                if path.is_dir() {
                    if let Some(tool) = Self::discover_mcp_tool(&path) {
                        tools.insert(tool.id.clone(), tool);
                    }
                    continue;
                }

                // Handle executable files
                if Self::is_executable(&path) {
                    if let Some(tool) = Self::discover_shell_tool(&path) {
                        tools.insert(tool.id.clone(), tool);
                    }
                }
            }
        }

        tracing::info!("Discovered {} security tools from {:?}", tools.len(), tools_dir);

        Self {
            tools_dir,
            tools,
            parallel_semaphore: None, // Sequential by default
        }
    }

    /// Enable parallel execution with a maximum concurrency limit
    pub fn with_parallel_execution(mut self, max_concurrent: usize) -> Self {
        self.parallel_semaphore = Some(Arc::new(Semaphore::new(max_concurrent)));
        self
    }

    /// Check if parallel execution is enabled
    pub fn is_parallel(&self) -> bool {
        self.parallel_semaphore.is_some()
    }

    /// Get the parallel semaphore if enabled
    pub fn semaphore(&self) -> Option<Arc<Semaphore>> {
        self.parallel_semaphore.clone()
    }

    /// Discover an MCP tool from a directory
    fn discover_mcp_tool(dir: &Path) -> Option<SecurityTool> {
        // Check for Cargo.toml (Rust MCP tool)
        let cargo_path = dir.join("Cargo.toml");
        if !cargo_path.exists() {
            return None;
        }

        // Check for tool.json metadata
        let metadata_path = dir.join("tool.json");
        let metadata = Self::read_metadata(&metadata_path);

        let dir_name = dir.file_name()?.to_str()?;
        let id = dir_name.trim_end_matches("-mcp").to_string();

        // Try to find the built binary
        let binary_path = dir.join("target/release").join(&id);
        let debug_binary_path = dir.join("target/debug").join(&id);
        
        let command_path = if binary_path.exists() {
            binary_path
        } else if debug_binary_path.exists() {
            debug_binary_path
        } else {
            // Return the cargo run command path
            dir.to_path_buf()
        };

        Some(SecurityTool {
            id: id.clone(),
            name: metadata.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| {
                id.replace('-', " ").replace('_', " ")
            }),
            description: metadata.as_ref().map(|m| m.description.clone()).unwrap_or_else(|| {
                format!("MCP security tool: {}", id)
            }),
            category: metadata.as_ref().map(|m| m.category.clone()).unwrap_or_default(),
            tags: metadata.as_ref().map(|m| m.tags.clone()).unwrap_or_default(),
            command_path,
            requires_sudo: metadata.as_ref().map(|m| m.requires_sudo).unwrap_or(false),
            timeout_secs: metadata.as_ref().and_then(|m| m.timeout_secs),
            args: metadata.map(|m| m.args).unwrap_or_default(),
        })
    }

    /// Discover a shell tool (script or binary)
    fn discover_shell_tool(path: &Path) -> Option<SecurityTool> {
        let file_name = path.file_name()?.to_str()?;
        
        // Skip known non-tool files
        if file_name.ends_with(".sh") && file_name.contains("setup") {
            return None;
        }
        if file_name.ends_with(".md") || file_name.ends_with(".json") {
            return None;
        }

        // Check for adjacent tool.json
        let metadata_path = path.with_extension("json");
        let metadata = Self::read_metadata(&metadata_path);

        let id = path.file_stem()?.to_str()?.to_string();

        Some(SecurityTool {
            id: id.clone(),
            name: metadata.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| {
                id.replace('-', " ").replace('_', " ")
            }),
            description: metadata.as_ref().map(|m| m.description.clone()).unwrap_or_else(|| {
                format!("Security tool: {}", id)
            }),
            category: metadata.as_ref().map(|m| m.category.clone()).unwrap_or_default(),
            tags: metadata.as_ref().map(|m| m.tags.clone()).unwrap_or_default(),
            command_path: path.to_path_buf(),
            requires_sudo: metadata.as_ref().map(|m| m.requires_sudo).unwrap_or(false),
            timeout_secs: metadata.as_ref().and_then(|m| m.timeout_secs),
            args: metadata.map(|m| m.args).unwrap_or_default(),
        })
    }

    /// Read tool.json metadata file
    fn read_metadata(path: &Path) -> Option<ToolMetadata> {
        if !path.exists() {
            return None;
        }

        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Check if a file is executable
    #[cfg(unix)]
    fn is_executable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = std::fs::metadata(path) {
            let permissions = metadata.permissions();
            permissions.mode() & 0o111 != 0
        } else {
            false
        }
    }

    #[cfg(not(unix))]
    fn is_executable(path: &Path) -> bool {
        path.extension()
            .map(|ext| ext == "exe" || ext == "bat" || ext == "cmd")
            .unwrap_or(false)
    }

    /// Get all discovered tools
    pub fn tools(&self) -> impl Iterator<Item = &SecurityTool> {
        self.tools.values()
    }

    /// Get a tool by ID
    pub fn get(&self, id: &str) -> Option<&SecurityTool> {
        self.tools.get(id)
    }

    /// Get tools by category
    pub fn by_category(&self, category: SecurityCategory) -> Vec<&SecurityTool> {
        self.tools.values().filter(|t| t.category == category).collect()
    }

    /// Get tools matching any of the specified tags.
    /// 
    /// If tags contains "all", returns all tools.
    /// Otherwise, returns tools that have at least one matching tag.
    pub fn by_tags(&self, tags: &[&str]) -> Vec<&SecurityTool> {
        // "all" tag means return everything
        if tags.iter().any(|t| t.eq_ignore_ascii_case("all")) {
            return self.tools.values().collect();
        }

        self.tools.values()
            .filter(|tool| {
                tool.tags.iter().any(|tool_tag| {
                    tags.iter().any(|filter_tag| tool_tag.eq_ignore_ascii_case(filter_tag))
                })
            })
            .collect()
    }

    /// Get all unique tags across all tools
    pub fn all_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self.tools.values()
            .flat_map(|t| t.tags.iter().cloned())
            .collect();
        tags.sort();
        tags.dedup();
        tags
    }

    /// Check if a tool has a specific tag
    pub fn has_tag(&self, tool_id: &str, tag: &str) -> bool {
        self.tools.get(tool_id)
            .map(|t| t.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)))
            .unwrap_or(false)
    }

    /// Get a formatted description of all tools for LLM consumption
    pub fn tool_descriptions(&self) -> String {
        let mut descriptions = Vec::new();
        
        // Group by category
        let mut by_category: HashMap<SecurityCategory, Vec<&SecurityTool>> = HashMap::new();
        for tool in self.tools.values() {
            by_category.entry(tool.category.clone()).or_default().push(tool);
        }

        for (category, tools) in by_category {
            descriptions.push(format!("\n=== {} Tools ===", category));
            for tool in tools {
                let args_desc = if tool.args.is_empty() {
                    String::new()
                } else {
                    let args: Vec<String> = tool.args.iter()
                        .map(|a| if a.required {
                            format!("  - {} (required): {}", a.name, a.description)
                        } else {
                            format!("  - {} (optional): {}", a.name, a.description)
                        })
                        .collect();
                    format!("\n  Arguments:\n{}", args.join("\n"))
                };
                
                descriptions.push(format!(
                    "• {} (id: {})\n  {}{}\n  Requires sudo: {}",
                    tool.name, tool.id, tool.description, args_desc, tool.requires_sudo
                ));
            }
        }

        descriptions.join("\n")
    }

    /// Execute a tool by ID with arguments
    pub fn execute(&self, tool_id: &str, args: &[String]) -> Result<ToolOutput> {
        let tool = self.tools.get(tool_id)
            .ok_or_else(|| crate::error::Error::tool_execution(
                tool_id,
                format!("Tool '{}' not found. Available tools: {:?}", 
                    tool_id, 
                    self.tools.keys().collect::<Vec<_>>())
            ))?;

        Ok(tool.execute(args))
    }

    /// Get the tools directory path
    pub fn tools_dir(&self) -> &Path {
        &self.tools_dir
    }

    /// Get the number of discovered tools
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// Tool that allows agents to list available security tools
pub struct ListSecurityTools {
    registry: Arc<SecurityToolRegistry>,
}

impl ListSecurityTools {
    pub fn new(registry: Arc<SecurityToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for ListSecurityTools {
    fn id(&self) -> &str {
        "list_security_tools"
    }

    fn name(&self) -> &str {
        "List Security Tools"
    }

    fn description(&self) -> &str {
        "List all available security tools that can be executed. \
         Returns tool names, IDs, descriptions, and categories."
    }

    fn input_schema(&self) -> JsonSchema {
        let mut properties = HashMap::new();
        properties.insert(
            "category".to_string(),
            serde_json::json!({
                "type": "string",
                "enum": ["network", "process", "rootkit", "hardening", "filesystem", "general"],
                "description": "Optional: filter by category"
            }),
        );
        JsonSchema::object(properties)
    }

    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let category_filter = params
            .get("category")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "network" => Some(SecurityCategory::Network),
                "process" => Some(SecurityCategory::Process),
                "rootkit" => Some(SecurityCategory::Rootkit),
                "hardening" => Some(SecurityCategory::Hardening),
                "filesystem" => Some(SecurityCategory::Filesystem),
                "general" => Some(SecurityCategory::General),
                _ => None,
            });

        let tools: Vec<&SecurityTool> = if let Some(cat) = category_filter {
            self.registry.by_category(cat)
        } else {
            self.registry.tools().collect()
        };

        if tools.is_empty() {
            return Ok(ToolOutput::success("No security tools found in the registry."));
        }

        let mut output = format!("Found {} security tools:\n\n", tools.len());
        for tool in tools {
            output.push_str(&format!(
                "• {} (id: '{}')\n  Category: {}\n  Description: {}\n  Sudo: {}\n\n",
                tool.name, tool.id, tool.category, tool.description, tool.requires_sudo
            ));
        }

        Ok(ToolOutput::success(output))
    }
}

/// Tool that allows agents to execute a security tool
pub struct RunSecurityTool {
    registry: Arc<SecurityToolRegistry>,
}

impl RunSecurityTool {
    pub fn new(registry: Arc<SecurityToolRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for RunSecurityTool {
    fn id(&self) -> &str {
        "run_security_tool"
    }

    fn name(&self) -> &str {
        "Run Security Tool"
    }

    fn description(&self) -> &str {
        "Execute a security tool from the registry by its ID. \
         Use list_security_tools first to see available tools and their IDs."
    }

    fn input_schema(&self) -> JsonSchema {
        let mut properties = HashMap::new();
        properties.insert(
            "tool_id".to_string(),
            serde_json::json!({
                "type": "string",
                "description": "The ID of the tool to execute (e.g., 'portlist', 'chkrootkit')"
            }),
        );
        properties.insert(
            "args".to_string(),
            serde_json::json!({
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional command-line arguments to pass to the tool"
            }),
        );
        JsonSchema::object(properties).with_required(vec!["tool_id".to_string()])
    }

    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let tool_id = params
            .get("tool_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::error::Error::InvalidInput("Missing 'tool_id' parameter".into()))?;

        let args: Vec<String> = params
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        tracing::info!("Executing security tool '{}' with args: {:?}", tool_id, args);

        self.registry.execute(tool_id, &args)
    }
}

/// Helper to create tools filtered by tags for agent use.
/// 
/// This creates ListSecurityTools and RunSecurityTool instances that only
/// expose tools matching the specified tags.
pub struct TaggedSecurityTools {
    registry: Arc<SecurityToolRegistry>,
    tags: Vec<String>,
}

impl TaggedSecurityTools {
    /// Create a new tagged tools helper.
    /// 
    /// # Arguments
    /// * `registry` - The security tool registry
    /// * `tags` - Tags to filter by. Use "all" to include all tools.
    /// 
    /// # Example
    /// ```ignore
    /// let registry = Arc::new(SecurityToolRegistry::discover("tools"));
    /// let security_tools = TaggedSecurityTools::new(registry.clone(), &["security_tools"]);
    /// let tools = security_tools.create_tools();
    /// ```
    pub fn new(registry: Arc<SecurityToolRegistry>, tags: &[&str]) -> Self {
        Self {
            registry,
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Get the filtered tools from the registry
    pub fn filtered_tools(&self) -> Vec<&SecurityTool> {
        let tag_refs: Vec<&str> = self.tags.iter().map(|s| s.as_str()).collect();
        self.registry.by_tags(&tag_refs)
    }

    /// Create ListSecurityTools and RunSecurityTool for agents.
    /// Returns a vector of Arc<dyn Tool> ready to add to an agent.
    pub fn create_tools(&self) -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(TaggedListSecurityTools::new(
                self.registry.clone(),
                self.tags.clone(),
            )) as Arc<dyn Tool>,
            Arc::new(TaggedRunSecurityTool::new(
                self.registry.clone(),
                self.tags.clone(),
            )) as Arc<dyn Tool>,
        ]
    }

    /// Get the tags this helper filters by
    pub fn tags(&self) -> &[String] {
        &self.tags
    }
}

/// List security tools filtered by tags
pub struct TaggedListSecurityTools {
    registry: Arc<SecurityToolRegistry>,
    tags: Vec<String>,
}

impl TaggedListSecurityTools {
    /// Create a new tagged list tools
    pub fn new(registry: Arc<SecurityToolRegistry>, tags: Vec<String>) -> Self {
        Self { registry, tags }
    }
}

#[async_trait]
impl Tool for TaggedListSecurityTools {
    fn id(&self) -> &str {
        "list_security_tools"
    }

    fn name(&self) -> &str {
        "List Security Tools"
    }

    fn description(&self) -> &str {
        "List available security tools that can be executed. \
         Returns tool names, IDs, descriptions, categories, and tags."
    }

    fn input_schema(&self) -> JsonSchema {
        let mut properties = HashMap::new();
        properties.insert(
            "category".to_string(),
            serde_json::json!({
                "type": "string",
                "enum": ["network", "process", "rootkit", "hardening", "filesystem", "general"],
                "description": "Optional: filter by category"
            }),
        );
        JsonSchema::object(properties)
    }

    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let category_filter = params
            .get("category")
            .and_then(|v| v.as_str())
            .and_then(|s| match s {
                "network" => Some(SecurityCategory::Network),
                "process" => Some(SecurityCategory::Process),
                "rootkit" => Some(SecurityCategory::Rootkit),
                "hardening" => Some(SecurityCategory::Hardening),
                "filesystem" => Some(SecurityCategory::Filesystem),
                "general" => Some(SecurityCategory::General),
                _ => None,
            });

        // First filter by tags
        let tag_refs: Vec<&str> = self.tags.iter().map(|s| s.as_str()).collect();
        let mut tools: Vec<&SecurityTool> = self.registry.by_tags(&tag_refs);

        // Then filter by category if specified
        if let Some(cat) = category_filter {
            tools.retain(|t| t.category == cat);
        }

        if tools.is_empty() {
            return Ok(ToolOutput::success(format!(
                "No security tools found matching tags: {:?}",
                self.tags
            )));
        }

        let mut output = format!("Found {} security tools:\n\n", tools.len());
        for tool in tools {
            let tags_str = if tool.tags.is_empty() {
                String::new()
            } else {
                format!("\n  Tags: {}", tool.tags.join(", "))
            };
            output.push_str(&format!(
                "• {} (id: '{}')\n  Category: {}\n  Description: {}\n  Sudo: {}{}\n\n",
                tool.name, tool.id, tool.category, tool.description, tool.requires_sudo, tags_str
            ));
        }

        Ok(ToolOutput::success(output))
    }
}

/// Run security tool filtered by tags
pub struct TaggedRunSecurityTool {
    registry: Arc<SecurityToolRegistry>,
    tags: Vec<String>,
}

impl TaggedRunSecurityTool {
    /// Create a new tagged run tool
    pub fn new(registry: Arc<SecurityToolRegistry>, tags: Vec<String>) -> Self {
        Self { registry, tags }
    }
}

#[async_trait]
impl Tool for TaggedRunSecurityTool {
    fn id(&self) -> &str {
        "run_security_tool"
    }

    fn name(&self) -> &str {
        "Run Security Tool"
    }

    fn description(&self) -> &str {
        "Execute a security tool from the registry by its ID. \
         Use list_security_tools first to see available tools and their IDs."
    }

    fn input_schema(&self) -> JsonSchema {
        let mut properties = HashMap::new();
        properties.insert(
            "tool_id".to_string(),
            serde_json::json!({
                "type": "string",
                "description": "The ID of the tool to execute (e.g., 'portlist', 'chkrootkit')"
            }),
        );
        properties.insert(
            "args".to_string(),
            serde_json::json!({
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional command-line arguments to pass to the tool"
            }),
        );
        JsonSchema::object(properties).with_required(vec!["tool_id".to_string()])
    }

    async fn execute(&self, params: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let tool_id = params
            .get("tool_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::error::Error::InvalidInput("Missing 'tool_id' parameter".into()))?;

        // Check if the tool is allowed by tags
        let tag_refs: Vec<&str> = self.tags.iter().map(|s| s.as_str()).collect();
        let allowed_tools = self.registry.by_tags(&tag_refs);
        
        if !allowed_tools.iter().any(|t| t.id == tool_id) {
            return Ok(ToolOutput::failure(format!(
                "Tool '{}' is not available with current tags: {:?}. \
                 Use list_security_tools to see available tools.",
                tool_id, self.tags
            )));
        }

        let args: Vec<String> = params
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        tracing::info!("Executing security tool '{}' with args: {:?}", tool_id, args);

        self.registry.execute(tool_id, &args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_category_display() {
        assert_eq!(format!("{}", SecurityCategory::Network), "network");
        assert_eq!(format!("{}", SecurityCategory::Rootkit), "rootkit");
    }

    #[test]
    fn test_tool_metadata_deserialize() {
        let json = r#"{
            "name": "Test Tool",
            "description": "A test tool",
            "category": "network",
            "tags": ["security_tools", "network_tools"],
            "requires_sudo": true,
            "args": [
                {"name": "verbose", "description": "Enable verbose output", "required": false}
            ]
        }"#;

        let metadata: ToolMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.name, "Test Tool");
        assert_eq!(metadata.category, SecurityCategory::Network);
        assert!(metadata.requires_sudo);
        assert_eq!(metadata.args.len(), 1);
        assert_eq!(metadata.tags, vec!["security_tools", "network_tools"]);
    }

    #[test]
    fn test_tool_metadata_deserialize_no_tags() {
        let json = r#"{
            "name": "Test Tool",
            "description": "A test tool"
        }"#;

        let metadata: ToolMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.tags.len(), 0);
    }

    #[test]
    fn test_registry_empty_dir() {
        let registry = SecurityToolRegistry::discover("/nonexistent/path");
        assert!(registry.is_empty());
    }
}

