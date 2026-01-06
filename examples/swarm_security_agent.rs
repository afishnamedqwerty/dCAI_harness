//! Swarm Security Agent - Multi-agent orchestration with DYNAMIC tool execution
//!
//! This agent uses a SecurityToolRegistry to dynamically discover and execute
//! security tools from the tools/ directory. Agents choose which tools to run
//! based on their specialized role.
//!
//! Workflow:
//! 1. Discover available security tools from tools/ directory
//! 2. Data Collector agent dynamically selects and runs appropriate tools
//! 3. Specialized analysis agents interpret the collected real data
//! 4. Coordinator synthesizes all findings
//! 5. Generate summary with verification commands

use spai::prelude::*;
use spai::react::Observation;
use spai::handoffs::HandoffContext;
use spai::security_tools::{SecurityToolRegistry, TaggedSecurityTools};
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;
use chrono::Utc;

/// Security findings collected and analyzed
#[derive(Debug, Clone, Default)]
struct SecurityFindings {
    collected_data: String,
    network_analysis: String,
    process_analysis: String,
    rootkit_analysis: String,
    hardening_analysis: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let output_dir = PathBuf::from(format!("security_swarm_{}", timestamp));
    fs::create_dir_all(&output_dir)?;

    println!("═══════════════════════════════════════════════════════════════");
    println!("   SPAI Swarm Security Agent - Dynamic Tool Execution");
    println!("   Output Directory: {}", output_dir.display());
    println!("═══════════════════════════════════════════════════════════════\n");

    // ═══════════════════════════════════════════════════════════════════════════
    // SETUP: Discover tools and create LLM client
    // ═══════════════════════════════════════════════════════════════════════════

    // Discover available security tools from tools/ directory
    let tools_dir = PathBuf::from("tools");
    let registry = Arc::new(SecurityToolRegistry::discover(&tools_dir));
    
    println!("✓ Discovered {} security tools from {:?}", registry.len(), tools_dir);
    println!("  Available tags: {:?}", registry.all_tags());
    for tool in registry.tools() {
        let tags = if tool.tags.is_empty() { 
            String::from("(no tags)") 
        } else { 
            tool.tags.join(", ") 
        };
        println!("  • {} ({}) - {} [{}]", tool.name, tool.id, tool.category, tags);
    }
    println!();

    // Create tools for agents using tag-based filtering
    // Agents with "security_tools" tag get access to security-related tools
    let tagged_tools = TaggedSecurityTools::new(registry.clone(), &["security_tools"]);
    let security_tools = tagged_tools.create_tools();
    
    println!("✓ Loaded {} tools with tag 'security_tools'", tagged_tools.filtered_tools().len());

    // Create OpenRouter client
    let client: Arc<dyn LlmClient> = match OpenRouterClient::from_env() {
        Ok(openrouter) => {
            println!("✓ Using OpenRouter API");
            Arc::new(openrouter)
        }
        Err(e) => {
            eprintln!("❌ OpenRouter client not available: {}", e);
            return Err(anyhow::anyhow!("OpenRouter API key required"));
        }
    };

    let model_id = "anthropic/claude-sonnet-4".to_string();
    println!("✓ Model: {}\n", model_id);

    let mut findings = SecurityFindings::default();
    let mut handoff_context = HandoffContext::new("Comprehensive security assessment");

    // ═══════════════════════════════════════════════════════════════════════════
    // PHASE 1: DATA COLLECTION (Agent-driven tool selection)
    // ═══════════════════════════════════════════════════════════════════════════
    println!("┌─────────────────────────────────────────────────────────────┐");
    println!("│  PHASE 1: Agent-Driven Security Data Collection            │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("  🔍 Data Collector agent selecting and running tools...");

    let collector_agent = Agent::builder()
        .name("Security Data Collector")
        .model(&model_id)
        .system_prompt(
            "You are a security data collection agent. Your job is to gather comprehensive \
             security data from the system using the available tools.\n\n\
             WORKFLOW:\n\
             1. First, call list_security_tools to see what tools are available\n\
             2. Run tools from each category to collect comprehensive data:\n\
                - Network: Check listening ports, established connections\n\
                - Process: Identify running processes, resource usage\n\
                - Rootkit: Scan for rootkits and suspicious modifications\n\
                - Hardening: Audit system security configuration\n\n\
             For each tool, use appropriate arguments. For example:\n\
             - portlist: use args [\"-a\", \"-s\"] for all connections with suspicious highlighting\n\
             - chkrootkit: no special args needed\n\n\
             Run at least one tool from each category. Collect all the output.\n\
             When done, provide a summary of what data you collected."
        )
        .tools(security_tools.clone())
        .max_loops(10)
        .temperature(0.1)
        .client(client.clone())
        .build()?;

    let collection_prompt = 
        "Perform a comprehensive security data collection. \
         First list available tools, then run tools from network, process, rootkit, \
         and hardening categories. Collect their output for analysis.";

    match collector_agent.react_loop(collection_prompt).await {
        Ok(output) => {
            findings.collected_data = output.content.clone();
            handoff_context = handoff_context.with_observation(Observation::new(
                format!("[collector] Collected security data from {} tool executions", 
                    output.trace.observations.len()),
            ));
            
            // Save raw collected data
            fs::write(output_dir.join("01_collected_data.txt"), &findings.collected_data)?;
            println!("     ✓ Data collection complete, saved to 01_collected_data.txt");
        }
        Err(e) => {
            findings.collected_data = format!("Collection failed: {}", e);
            println!("     ⚠️ Data collection failed: {}", e);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // PHASE 2: AGENT ANALYSIS (Specialists analyze collected data)
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│  PHASE 2: Multi-Agent Analysis of Collected Data           │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    // Network Analysis Agent
    println!("  📡 Network Monitor analyzing collected data...");
    let network_agent = Agent::builder()
        .name("Network Monitor")
        .model(&model_id)
        .system_prompt(
            "You are a network security analyst. You will receive security data collected \
             from the system.\n\n\
             CRITICAL: Analyze ONLY the data provided. Do NOT invent any findings.\n\n\
             Look for:\n\
             - Suspicious listening ports (4444, 31337, 6667, 1337, etc.)\n\
             - Unusual established connections to unknown IPs\n\
             - Processes with unexpected network activity\n\
             - Any ports flagged as SUSPICIOUS\n\n\
             Provide a brief analysis and list any suspicious PIDs.\n\
             End with: SUSPICIOUS_PIDS: [list] or SUSPICIOUS_PIDS: NONE"
        )
        .temperature(0.1)
        .client(client.clone())
        .build()?;

    let network_prompt = format!(
        "Analyze the NETWORK-related data from this security collection:\n\n{}\n\n\
         Focus on port scans, network connections, and related findings.",
        truncate_str(&findings.collected_data, 8000)
    );

    match network_agent.react_loop(&network_prompt).await {
        Ok(output) => {
            findings.network_analysis = output.content.clone();
            handoff_context = handoff_context.with_observation(Observation::new(
                format!("[network] {}", truncate_str(&output.content, 500)),
            ));
            fs::write(output_dir.join("02_network_analysis.txt"), &output.content)?;
            println!("     ✓ Network analysis complete");
        }
        Err(e) => {
            findings.network_analysis = format!("Analysis failed: {}", e);
            println!("     ⚠️ Network analysis failed: {}", e);
        }
    }

    // Process Analysis Agent
    println!("  🔍 Process Analyzer analyzing collected data...");
    let process_agent = Agent::builder()
        .name("Process Analyzer")
        .model(&model_id)
        .system_prompt(
            "You are a process analyst. Analyze ONLY the data provided.\n\n\
             Look for:\n\
             - Suspicious process names or paths\n\
             - Processes running from /tmp or /dev/shm\n\
             - Abnormally high CPU or memory usage\n\
             - Unusual parent-child relationships\n\n\
             End with: SEVERITY: [CLEAN|LOW|MEDIUM|HIGH|CRITICAL] - [reason]"
        )
        .temperature(0.1)
        .client(client.clone())
        .build()?;

    let process_prompt = format!(
        "Analyze the PROCESS-related data from this security collection:\n\n{}\n\n\
         Previous network analysis found:\n{}",
        truncate_str(&findings.collected_data, 6000),
        truncate_str(&findings.network_analysis, 1000)
    );

    match process_agent.react_loop(&process_prompt).await {
        Ok(output) => {
            findings.process_analysis = output.content.clone();
            handoff_context = handoff_context.with_observation(Observation::new(
                format!("[process] {}", truncate_str(&output.content, 500)),
            ));
            fs::write(output_dir.join("03_process_analysis.txt"), &output.content)?;
            println!("     ✓ Process analysis complete");
        }
        Err(e) => {
            findings.process_analysis = format!("Analysis failed: {}", e);
            println!("     ⚠️ Process analysis failed: {}", e);
        }
    }

    // Rootkit Analysis Agent
    println!("  🦠 Rootkit Hunter analyzing collected data...");
    let rootkit_agent = Agent::builder()
        .name("Rootkit Hunter")
        .model(&model_id)
        .system_prompt(
            "You are a rootkit analyst. Analyze ONLY the data provided.\n\n\
             Look for:\n\
             - Any 'INFECTED' or 'WARNING' messages\n\
             - Hidden files or processes detected\n\
             - Modified system binaries\n\
             - Suspicious kernel modules\n\n\
             End with: ROOTKIT_STATUS: [CLEAN|WARNING|INFECTED] - [reason]"
        )
        .temperature(0.1)
        .client(client.clone())
        .build()?;

    let rootkit_prompt = format!(
        "Analyze the ROOTKIT scan data from this security collection:\n\n{}",
        truncate_str(&findings.collected_data, 8000)
    );

    match rootkit_agent.react_loop(&rootkit_prompt).await {
        Ok(output) => {
            findings.rootkit_analysis = output.content.clone();
            handoff_context = handoff_context.with_observation(Observation::new(
                format!("[rootkit] {}", truncate_str(&output.content, 500)),
            ));
            fs::write(output_dir.join("04_rootkit_analysis.txt"), &output.content)?;
            println!("     ✓ Rootkit analysis complete");
        }
        Err(e) => {
            findings.rootkit_analysis = format!("Analysis failed: {}", e);
            println!("     ⚠️ Rootkit analysis failed: {}", e);
        }
    }

    // Hardening Analysis Agent
    println!("  🛡️  Hardening Auditor analyzing collected data...");
    let hardening_agent = Agent::builder()
        .name("Hardening Auditor")
        .model(&model_id)
        .system_prompt(
            "You are a system hardening auditor. Analyze ONLY the data provided.\n\n\
             Extract:\n\
             - The actual hardening index score from any lynis output\n\
             - Top 5 security warnings or suggestions\n\n\
             End with: HARDENING_SCORE: [score from output] - [top recommendation]"
        )
        .temperature(0.1)
        .client(client.clone())
        .build()?;

    let hardening_prompt = format!(
        "Analyze the HARDENING/LYNIS data from this security collection:\n\n{}",
        truncate_str(&findings.collected_data, 8000)
    );

    match hardening_agent.react_loop(&hardening_prompt).await {
        Ok(output) => {
            findings.hardening_analysis = output.content.clone();
            handoff_context = handoff_context.with_observation(Observation::new(
                format!("[hardening] {}", truncate_str(&output.content, 500)),
            ));
            fs::write(output_dir.join("05_hardening_analysis.txt"), &output.content)?;
            println!("     ✓ Hardening analysis complete");
        }
        Err(e) => {
            findings.hardening_analysis = format!("Analysis failed: {}", e);
            println!("     ⚠️ Hardening analysis failed: {}", e);
        }
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // PHASE 3: COORDINATOR SYNTHESIS
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│  PHASE 3: Coordinator Synthesis                             │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    println!("  🎯 Coordinator synthesizing all findings...");
    let coordinator_agent = Agent::builder()
        .name("Security Coordinator")
        .model(&model_id)
        .system_prompt(
            "You are the security coordinator. Synthesize the analyses from all agents.\n\n\
             CRITICAL: Base your assessment ONLY on the agent analyses provided.\n\n\
             Provide:\n\
             1. EXECUTIVE SUMMARY (2-3 sentences)\n\
             2. SECURITY POSTURE: [SECURE|WARNING|COMPROMISED]\n\
             3. HIGH SEVERITY FINDINGS (if any, with specific PIDs/IPs/ports)\n\
             4. VERIFICATION COMMANDS - bash commands to verify the highest severity findings\n\n\
             Format verification commands as:\n\
             ```bash\n\
             # Description of what this verifies\n\
             command here\n\
             ```"
        )
        .temperature(0.1)
        .client(client.clone())
        .build()?;

    let coordinator_prompt = format!(
        "Synthesize these security findings from our agent swarm:\n\n\
         === NETWORK ANALYSIS ===\n{}\n\n\
         === PROCESS ANALYSIS ===\n{}\n\n\
         === ROOTKIT ANALYSIS ===\n{}\n\n\
         === HARDENING ANALYSIS ===\n{}\n\n\
         Provide executive summary, security posture, and verification bash commands.",
        truncate_str(&findings.network_analysis, 1500),
        truncate_str(&findings.process_analysis, 1500),
        truncate_str(&findings.rootkit_analysis, 1500),
        truncate_str(&findings.hardening_analysis, 1500)
    );

    let final_assessment = match coordinator_agent.react_loop(&coordinator_prompt).await {
        Ok(output) => output.content,
        Err(e) => format!("Coordinator failed: {}", e),
    };

    // ═══════════════════════════════════════════════════════════════════════════
    // PHASE 4: GENERATE SUMMARY FILE
    // ═══════════════════════════════════════════════════════════════════════════
    println!("\n┌─────────────────────────────────────────────────────────────┐");
    println!("│  PHASE 4: Generating Summary                                │");
    println!("└─────────────────────────────────────────────────────────────┘\n");

    let summary = format!(
        "═══════════════════════════════════════════════════════════════\n\
         SPAI SWARM SECURITY ASSESSMENT SUMMARY\n\
         Generated: {}\n\
         Tools Directory: {:?}\n\
         ═══════════════════════════════════════════════════════════════\n\n\
         {}\n\n\
         ═══════════════════════════════════════════════════════════════\n\
         ANALYSIS FILES\n\
         ═══════════════════════════════════════════════════════════════\n\
         cat {}/01_collected_data.txt\n\
         cat {}/02_network_analysis.txt\n\
         cat {}/03_process_analysis.txt\n\
         cat {}/04_rootkit_analysis.txt\n\
         cat {}/05_hardening_analysis.txt\n\n\
         ═══════════════════════════════════════════════════════════════\n\
         AGENT ANALYSES\n\
         ═══════════════════════════════════════════════════════════════\n\n\
         --- Network Agent ---\n{}\n\n\
         --- Process Agent ---\n{}\n\n\
         --- Rootkit Agent ---\n{}\n\n\
         --- Hardening Agent ---\n{}\n\n\
         ═══════════════════════════════════════════════════════════════\n\
         QUICK VERIFICATION COMMANDS\n\
         ═══════════════════════════════════════════════════════════════\n\n\
         # Check listening ports\n\
         ss -tulnp | grep LISTEN\n\n\
         # Check high CPU processes\n\
         ps aux --sort=-%cpu | head -10\n\n\
         # Check for suspicious network connections\n\
         lsof -i -n -P | grep ESTABLISHED\n\n\
         # Run quick rootkit check\n\
         sudo chkrootkit | grep -i infected\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
        registry.tools_dir(),
        final_assessment,
        output_dir.display(), output_dir.display(), output_dir.display(), 
        output_dir.display(), output_dir.display(),
        truncate_str(&findings.network_analysis, 1000),
        truncate_str(&findings.process_analysis, 1000),
        truncate_str(&findings.rootkit_analysis, 1000),
        truncate_str(&findings.hardening_analysis, 1000)
    );

    fs::write(output_dir.join("summary.txt"), &summary)?;
    println!("   ✓ Summary saved to {}/summary.txt", output_dir.display());

    // Print final summary
    println!("\n═══════════════════════════════════════════════════════════════");
    println!("   ASSESSMENT COMPLETE");
    println!("   Output: {}/", output_dir.display());
    println!("═══════════════════════════════════════════════════════════════\n");

    println!("{}", final_assessment);

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("   View full summary: cat {}/summary.txt", output_dir.display());
    println!("═══════════════════════════════════════════════════════════════\n");

    Ok(())
}

fn truncate_str(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
