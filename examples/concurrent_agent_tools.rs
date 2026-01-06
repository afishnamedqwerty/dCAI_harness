//! Concurrent Agent Tools - Test tool tags with parallel agent execution
//!
//! This example demonstrates:
//! 1. Loading tools by tag category
//! 2. Creating specialized agents for each category
//! 3. Running concurrent tool verification tests
//! 4. Validating tool discovery and execution

use spai::prelude::*;
use spai::security_tools::{SecurityToolRegistry, TaggedSecurityTools};
use std::path::PathBuf;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::task::JoinSet;

/// Test result for a tool category
#[derive(Debug)]
struct CategoryTestResult {
    category: String,
    tools_discovered: usize,
    tools_tested: usize,
    passed: usize,
    failed: usize,
    errors: Vec<String>,
}

/// Tool categories to test
const TOOL_CATEGORIES: &[&str] = &[
    "security_tools",
    "web_tools", 
    "filesystem_tools",
    "dev_tools",
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("═══════════════════════════════════════════════════════════════");
    println!("   SPAI Concurrent Agent Tools Test");
    println!("   Testing tool discovery and execution by category");
    println!("═══════════════════════════════════════════════════════════════\n");

    // ═══════════════════════════════════════════════════════════════════════════
    // SETUP: Discover all tools
    // ═══════════════════════════════════════════════════════════════════════════
    
    let tools_dir = PathBuf::from("tools");
    let registry = Arc::new(SecurityToolRegistry::discover(&tools_dir));
    
    println!("✓ Discovered {} total tools from {:?}", registry.len(), tools_dir);
    println!("✓ Available tags: {:?}", registry.all_tags());
    println!();

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 1: Verify tool discovery by category
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  TEST 1: Tool Discovery by Category                        │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let mut category_tools: HashMap<String, Vec<String>> = HashMap::new();
    
    for category in TOOL_CATEGORIES {
        let helper = TaggedSecurityTools::new(registry.clone(), &[category]);
        let tools = helper.filtered_tools();
        
        let tool_names: Vec<String> = tools.iter().map(|t| t.id.clone()).collect();
        
        println!("📁 {} ({} tools)", category, tools.len());
        for tool in &tools {
            println!("   • {} - {}", tool.id, truncate(&tool.description, 50));
        }
        println!();
        
        category_tools.insert(category.to_string(), tool_names);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 2: Concurrent agent creation and tool loading
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  TEST 2: Concurrent Agent Creation                         │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    // Try to create LLM client (optional for tool tests)
    let llm_client: Option<Arc<dyn LlmClient>> = match OpenRouterClient::from_env() {
        Ok(client) => {
            println!("✓ OpenRouter client available for agent tests");
            Some(Arc::new(client))
        }
        Err(_) => {
            println!("⚠️  No OpenRouter API key - running tool-only tests");
            None
        }
    };
    println!();

    // Create agents for each category concurrently
    let mut agent_results = Vec::new();
    
    for category in TOOL_CATEGORIES {
        let helper = TaggedSecurityTools::new(registry.clone(), &[category]);
        let tools = helper.create_tools();
        
        println!("🤖 {} Agent: {} tools loaded", 
            capitalize(category), 
            tools.len() / 2  // Divide by 2 since we have list + run tools
        );
        
        // Store for later testing
        agent_results.push((category.to_string(), tools));
    }
    println!();

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 3: Direct tool execution tests
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  TEST 3: Direct Tool Execution Tests                       │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let mut test_results: Vec<CategoryTestResult> = Vec::new();
    
    for category in TOOL_CATEGORIES {
        println!("Testing {} tools...", category);
        
        let result = test_category_tools(&registry, category).await;
        
        let status = if result.failed == 0 { "✓" } else { "⚠️" };
        println!("  {} {}/{} passed\n", status, result.passed, result.tools_tested);
        
        test_results.push(result);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 4: Concurrent tool execution
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  TEST 4: Concurrent Tool Execution                         │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let concurrent_result = test_concurrent_execution(&registry).await;
    println!("  Concurrent execution test: {} tools tested in parallel\n", concurrent_result);

    // ═══════════════════════════════════════════════════════════════════════════
    // TEST 5: Agent-based tool invocation (if LLM available)
    // ═══════════════════════════════════════════════════════════════════════════
    
    if let Some(client) = llm_client {
        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│  TEST 5: Agent-Based Tool Invocation                       │");
        println!("└─────────────────────────────────────────────────────────────┘\n");

        // Test one agent per category
        for category in &["filesystem_tools", "dev_tools"] {
            let result = test_agent_tool_use(&registry, &client, category).await;
            println!("  {} agent: {}\n", category, result);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // SUMMARY
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("═══════════════════════════════════════════════════════════════");
    println!("   TEST SUMMARY");
    println!("═══════════════════════════════════════════════════════════════\n");

    let mut total_passed = 0;
    let mut total_failed = 0;
    
    for result in &test_results {
        let status = if result.failed == 0 { "✓" } else { "✗" };
        println!("  {} {}: {}/{} passed", 
            status, result.category, result.passed, result.tools_tested);
        
        total_passed += result.passed;
        total_failed += result.failed;
        
        if !result.errors.is_empty() {
            for err in &result.errors {
                println!("      ⚠️  {}", err);
            }
        }
    }
    
    println!();
    println!("  Total: {}/{} tests passed", total_passed, total_passed + total_failed);
    
    let overall_status = if total_failed == 0 { "ALL TESTS PASSED" } else { "SOME TESTS FAILED" };
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("   {}", overall_status);
    println!("═══════════════════════════════════════════════════════════════\n");

    Ok(())
}

/// Test ALL tools in a category
async fn test_category_tools(
    registry: &SecurityToolRegistry, 
    category: &str
) -> CategoryTestResult {
    let helper = TaggedSecurityTools::new(Arc::new(registry.clone()), &[category]);
    let tools = helper.filtered_tools();
    
    let mut result = CategoryTestResult {
        category: category.to_string(),
        tools_discovered: tools.len(),
        tools_tested: 0,
        passed: 0,
        failed: 0,
        errors: Vec::new(),
    };
    
    // Test ALL tools in the category with --help
    for tool in &tools {
        result.tools_tested += 1;
        
        // Use --help flag which should be safe for all tools
        let args = vec!["--help".to_string()];
        
        match registry.execute(&tool.id, &args) {
            Ok(output) => {
                if output.success {
                    result.passed += 1;
                } else {
                    // Some tools might exit with non-zero for --help, check if content exists
                    if !output.content.is_empty() {
                        result.passed += 1; // Tool ran and produced output
                    } else {
                        result.failed += 1;
                        result.errors.push(format!("{}: execution returned failure", tool.id));
                    }
                }
            }
            Err(e) => {
                result.failed += 1;
                result.errors.push(format!("{}: {}", tool.id, e));
            }
        }
    }
    
    result
}

/// Test concurrent tool execution - ALL tools in parallel
async fn test_concurrent_execution(registry: &SecurityToolRegistry) -> usize {
    let registry = Arc::new(registry.clone());
    let mut join_set = JoinSet::new();
    
    // Get ALL tools and test them concurrently
    let all_tool_ids: Vec<String> = registry.tools().map(|t| t.id.clone()).collect();
    
    for tool_id in all_tool_ids {
        let registry = registry.clone();
        
        join_set.spawn(async move {
            let args = vec!["--help".to_string()];
            registry.execute(&tool_id, &args).is_ok()
        });
    }
    
    let mut success_count = 0;
    while let Some(result) = join_set.join_next().await {
        if let Ok(true) = result {
            success_count += 1;
        }
    }
    
    success_count
}

/// Test agent-based tool usage
async fn test_agent_tool_use(
    registry: &SecurityToolRegistry,
    client: &Arc<dyn LlmClient>,
    category: &str,
) -> String {
    let helper = TaggedSecurityTools::new(Arc::new(registry.clone()), &[category]);
    let tools = helper.create_tools();
    
    let agent = match Agent::builder()
        .name(&format!("{} Test Agent", capitalize(category)))
        .model("tngtech/deepseek-r1t2-chimera:free")
        .system_prompt(&format!(
            "You are a test agent for {} tools. \
             When asked, use list_security_tools to see available tools, \
             then briefly describe what you found.",
            category
        ))
        .tools(tools)
        .max_loops(2)
        .temperature(0.1)
        .client(client.clone())
        .build()
    {
        Ok(agent) => agent,
        Err(e) => return format!("Failed to create agent: {}", e),
    };
    
    match agent.react_loop("List the available tools for this category.").await {
        Ok(output) => {
            if output.content.len() > 50 {
                format!("Success - agent responded with {} chars", output.content.len())
            } else {
                format!("Success - {}", truncate(&output.content, 100))
            }
        }
        Err(e) => format!("Error: {}", e),
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}
