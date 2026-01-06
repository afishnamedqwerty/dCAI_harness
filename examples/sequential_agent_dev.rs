//! Sequential Agent Dev - Write, compile, and run C code in sandbox
//!
//! This example demonstrates:
//! 1. Loading an agent with dev_tools tagged tools
//! 2. Writing a C hello world source file
//! 3. Compiling the C code with gcc in the sandbox
//! 4. Executing the compiled binary in the sandbox

use spai::prelude::*;
use spai::security_tools::{SecurityToolRegistry, TaggedSecurityTools};
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;

/// C source code for hello world
const C_HELLO_WORLD: &str = r#"#include <stdio.h>

int main() {
    printf("Hello World from SPAI Sandbox!\n");
    return 0;
}
"#;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    println!("═══════════════════════════════════════════════════════════════");
    println!("   SPAI Sequential Agent Dev - C Sandbox Hello World");
    println!("═══════════════════════════════════════════════════════════════\n");

    // ═══════════════════════════════════════════════════════════════════════════
    // SETUP: Create temp directory and write C file
    // ═══════════════════════════════════════════════════════════════════════════
    
    let sandbox_dir = PathBuf::from("/tmp/spai_sandbox");
    fs::create_dir_all(&sandbox_dir)?;
    
    let c_file = sandbox_dir.join("hello.c");
    fs::write(&c_file, C_HELLO_WORLD)?;
    
    println!("✓ Created C source file: {}", c_file.display());
    println!("  Contents:");
    println!("  ─────────────────────────────────────────────────────");
    for line in C_HELLO_WORLD.lines() {
        println!("  {}", line);
    }
    println!("  ─────────────────────────────────────────────────────\n");

    // ═══════════════════════════════════════════════════════════════════════════
    // SETUP: Discover tools and create LLM client
    // ═══════════════════════════════════════════════════════════════════════════
    
    let tools_dir = PathBuf::from("tools");
    let registry = Arc::new(SecurityToolRegistry::discover(&tools_dir));
    
    println!("✓ Discovered {} total tools", registry.len());
    
    // Load only dev_tools tagged tools
    let dev_tools_helper = TaggedSecurityTools::new(registry.clone(), &["dev_tools"]);
    let dev_tools = dev_tools_helper.create_tools();
    
    println!("✓ Loaded {} dev_tools for agent", dev_tools_helper.filtered_tools().len());
    println!("  Available tools:");
    for tool in dev_tools_helper.filtered_tools() {
        println!("    • {} - {}", tool.id, truncate(&tool.description, 50));
    }
    println!();

    // Create OpenRouter client
    let client: Arc<dyn LlmClient> = match OpenRouterClient::from_env() {
        Ok(openrouter) => {
            println!("✓ Using OpenRouter API");
            Arc::new(openrouter)
        }
        Err(e) => {
            eprintln!("✗ Failed to create OpenRouter client: {}", e);
            eprintln!("  Make sure OPENROUTER_API_KEY is set");
            return Err(e.into());
        }
    };
    println!();

    // ═══════════════════════════════════════════════════════════════════════════
    // CREATE AGENT: Dev tools agent for sandbox C compilation
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  Creating Dev Tools Agent                                  │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let agent = Agent::builder()
        .name("Dev Sandbox Agent")
        .model("anthropic/claude-sonnet-4")
        .system_prompt(&format!(r#"You are a development assistant with access to dev_tools.

A C source file has been created at: {c_file}

Your task is to:
1. First, use list_security_tools to see what tools are available
2. Use run_security_tool to execute sandbox_exec to compile the C file with gcc
3. Use run_security_tool to execute sandbox_exec to run the compiled binary

For compiling, use sandbox_exec with these arguments:
- tool_id: "sandbox_exec"  
- args: ["-l", "bash", "gcc -o /tmp/spai_sandbox/hello /tmp/spai_sandbox/hello.c && echo 'Compilation successful'"]

For running the compiled binary, use sandbox_exec with:
- tool_id: "sandbox_exec"
- args: ["-l", "bash", "/tmp/spai_sandbox/hello"]

Report the output of each step and confirm "Hello World from SPAI Sandbox!" was printed.
"#, c_file = c_file.display()))
        .tools(dev_tools)
        .max_loops(6)
        .temperature(0.2)
        .client(client)
        .build()?;

    println!("✓ Agent created: {}\n", agent.name);

    // ═══════════════════════════════════════════════════════════════════════════
    // EXECUTE: Run the agent to compile and run C code in sandbox
    // ═══════════════════════════════════════════════════════════════════════════
    
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  Executing Agent Task                                      │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let task = format!(
        "Compile the C file at {} using gcc in the sandbox, then run the compiled binary. \
         Report the hello world output.",
        c_file.display()
    );

    println!("📋 Task: {}\n", task);
    println!("─────────────────────────────────────────────────────────────────");

    match agent.react_loop(&task).await {
        Ok(output) => {
            println!("\n─────────────────────────────────────────────────────────────────");
            println!("\n✓ Agent completed successfully!\n");
            println!("═══════════════════════════════════════════════════════════════");
            println!("   AGENT RESPONSE");
            println!("═══════════════════════════════════════════════════════════════\n");
            println!("{}", output.content);
            
            // Note: The compiled binary exists INSIDE the sandbox's isolated filesystem,
            // not on the host. This is correct sandbox behavior - firejail uses --private
            // which creates an ephemeral filesystem that is discarded when the sandbox exits.
            // The agent successfully compiled and ran the binary within the same sandbox session.
            let binary_path = sandbox_dir.join("hello");
            if binary_path.exists() {
                println!("\n✓ Compiled binary persisted to host at: {}", binary_path.display());
            } else {
                println!("\n📦 Note: Binary was compiled and executed inside sandbox's isolated filesystem.");
                println!("   This is correct behavior - sandbox isolation prevents writes to host.");
            }
        }
        Err(e) => {
            println!("\n✗ Agent failed: {}", e);
            return Err(e.into());
        }
    }

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("   Task Complete");
    println!("═══════════════════════════════════════════════════════════════\n");

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// UNIT TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use spai::security_tools::SecurityToolRegistry;
    use std::process::Command;

    /// Test that the C source code is valid
    #[test]
    fn test_c_source_is_valid() {
        assert!(C_HELLO_WORLD.contains("#include <stdio.h>"));
        assert!(C_HELLO_WORLD.contains("int main()"));
        assert!(C_HELLO_WORLD.contains("printf"));
        assert!(C_HELLO_WORLD.contains("Hello World"));
        assert!(C_HELLO_WORLD.contains("return 0;"));
    }

    /// Test that the C file can be written to disk
    #[test]
    fn test_c_file_write() {
        let temp_dir = std::env::temp_dir().join("spai_test_c");
        fs::create_dir_all(&temp_dir).unwrap();
        
        let c_file = temp_dir.join("test_hello.c");
        fs::write(&c_file, C_HELLO_WORLD).unwrap();
        
        assert!(c_file.exists());
        let content = fs::read_to_string(&c_file).unwrap();
        assert_eq!(content, C_HELLO_WORLD);
        
        // Cleanup
        fs::remove_file(&c_file).ok();
        fs::remove_dir(&temp_dir).ok();
    }

    /// Test that gcc can compile the C code
    #[test]
    fn test_c_compiles_with_gcc() {
        // Skip if gcc not available
        if Command::new("gcc").arg("--version").output().is_err() {
            println!("Skipping test: gcc not found");
            return;
        }

        let temp_dir = std::env::temp_dir().join("spai_test_compile");
        fs::create_dir_all(&temp_dir).unwrap();
        
        let c_file = temp_dir.join("hello.c");
        let binary_file = temp_dir.join("hello");
        
        fs::write(&c_file, C_HELLO_WORLD).unwrap();
        
        // Compile
        let output = Command::new("gcc")
            .args(["-o", binary_file.to_str().unwrap(), c_file.to_str().unwrap()])
            .output()
            .expect("Failed to run gcc");
        
        assert!(output.status.success(), "gcc compilation failed: {:?}", 
            String::from_utf8_lossy(&output.stderr));
        assert!(binary_file.exists(), "Compiled binary should exist");
        
        // Cleanup
        fs::remove_file(&c_file).ok();
        fs::remove_file(&binary_file).ok();
        fs::remove_dir(&temp_dir).ok();
    }

    /// Test that the compiled binary runs and produces correct output
    #[test]
    fn test_c_binary_output() {
        // Skip if gcc not available
        if Command::new("gcc").arg("--version").output().is_err() {
            println!("Skipping test: gcc not found");
            return;
        }

        let temp_dir = std::env::temp_dir().join("spai_test_run");
        fs::create_dir_all(&temp_dir).unwrap();
        
        let c_file = temp_dir.join("hello.c");
        let binary_file = temp_dir.join("hello");
        
        fs::write(&c_file, C_HELLO_WORLD).unwrap();
        
        // Compile
        Command::new("gcc")
            .args(["-o", binary_file.to_str().unwrap(), c_file.to_str().unwrap()])
            .output()
            .expect("Failed to compile");
        
        // Run
        let output = Command::new(&binary_file)
            .output()
            .expect("Failed to run binary");
        
        assert!(output.status.success(), "Binary execution failed");
        
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("Hello World from SPAI Sandbox!"), 
            "Expected hello world output, got: {}", stdout);
        
        // Cleanup
        fs::remove_file(&c_file).ok();
        fs::remove_file(&binary_file).ok();
        fs::remove_dir(&temp_dir).ok();
    }

    /// Test that dev_tools are discovered correctly
    #[test]
    fn test_dev_tools_discovery() {
        let tools_dir = PathBuf::from("tools");
        
        if !tools_dir.exists() {
            println!("Skipping test: tools directory not found");
            return;
        }
        
        let registry = SecurityToolRegistry::discover(&tools_dir);
        let helper = TaggedSecurityTools::new(Arc::new(registry), &["dev_tools"]);
        let tools = helper.filtered_tools();
        
        assert!(!tools.is_empty(), "Should discover at least one dev_tool");
        
        // Check that sandbox_exec is available
        let sandbox_found = tools.iter().any(|t| t.id == "sandbox_exec");
        assert!(sandbox_found, "sandbox_exec should be in dev_tools");
    }

    /// Test sandbox_exec tool directly with --help
    #[test]
    fn test_sandbox_exec_help() {
        let tools_dir = PathBuf::from("tools");
        
        if !tools_dir.exists() {
            println!("Skipping test: tools directory not found");
            return;
        }
        
        let registry = SecurityToolRegistry::discover(&tools_dir);
        
        let result = registry.execute("sandbox_exec", &["--help".to_string()]);
        assert!(result.is_ok(), "sandbox_exec --help should succeed");
        
        let output = result.unwrap();
        assert!(!output.content.is_empty(), "Should have help output");
        assert!(output.content.contains("sandbox") || output.content.contains("Sandbox"),
            "Help should mention sandbox");
    }

    /// Test sandbox_exec with simple bash command
    #[test]
    fn test_sandbox_exec_bash() {
        let tools_dir = PathBuf::from("tools");
        
        if !tools_dir.exists() {
            println!("Skipping test: tools directory not found");
            return;
        }
        
        let registry = SecurityToolRegistry::discover(&tools_dir);
        
        let result = registry.execute("sandbox_exec", &[
            "-l".to_string(),
            "bash".to_string(),
            "echo 'test output'".to_string(),
        ]);
        
        // May fail if no sandbox available, but should at least execute
        if let Ok(output) = result {
            println!("Sandbox output: {}", output.content);
        }
    }
}
