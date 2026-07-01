use raios_core::security::{quarantine, secret_lease};

pub fn cmd_quarantine(action: raios_surface_cli::cli::QuarantineAction, json: bool) {
    use raios_surface_cli::cli::QuarantineAction::*;

    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {e}");
            std::process::exit(1);
        }
    };
    let _ = quarantine::ensure_table(&conn);

    match action {
        List => {
            let items = quarantine::list_pending(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&items).unwrap_or_default());
                return;
            }
            if items.is_empty() {
                println!("No pending quarantine items.");
                return;
            }
            println!("{:<20}  {:<25}  CREATED", "ID", "TOOL");
            for i in &items {
                println!("{:<20}  {:<25}  {}", i.id, i.tool, i.created_at);
            }
        }
        All => {
            let items = quarantine::list_all(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&items).unwrap_or_default());
                return;
            }
            if items.is_empty() {
                println!("No quarantine items found.");
                return;
            }
            println!("{:<20}  {:<25}  {:<10}  CREATED", "ID", "TOOL", "STATUS");
            for i in &items {
                println!(
                    "{:<20}  {:<25}  {:<10}  {}",
                    i.id, i.tool, i.status, i.created_at
                );
            }
        }
        Approve { id } => match quarantine::approve(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"approved\",\"id\":\"{id}\"}}");
                } else {
                    println!("Approved {id}. Agent may now retry the tool call.");
                }
            }
            Ok(false) => {
                eprintln!("No pending item with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("DB error: {e}");
                std::process::exit(1);
            }
        },
        Deny { id } => match quarantine::deny(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"denied\",\"id\":\"{id}\"}}");
                } else {
                    println!("Denied {id}. Future calls for this tool will be blocked.");
                }
            }
            Ok(false) => {
                eprintln!("No active item with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("DB error: {e}");
                std::process::exit(1);
            }
        },
        Clear { id } => match quarantine::clear(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"cleared\",\"id\":\"{id}\"}}");
                } else {
                    println!("Cleared {id} from quarantine queue.");
                }
            }
            Ok(false) => {
                eprintln!("No item with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("DB error: {e}");
                std::process::exit(1);
            }
        },
    }
}

pub fn cmd_secret(action: raios_surface_cli::cli::SecretAction, json: bool) {
    use raios_surface_cli::cli::SecretAction::*;

    let conn = match raios_core::db::open_db() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open database: {e}");
            std::process::exit(1);
        }
    };
    let _ = secret_lease::ensure_table(&conn);

    match action {
        Grant { tool, env_var, ttl } => {
            let ttl_secs = match secret_lease::parse_ttl(&ttl) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Invalid TTL: {e}");
                    std::process::exit(1);
                }
            };
            match secret_lease::grant(&conn, &tool, &env_var, ttl_secs) {
                Ok(id) => {
                    if json {
                        println!("{{\"status\":\"granted\",\"id\":\"{id}\",\"tool\":\"{tool}\",\"env_var\":\"{env_var}\",\"ttl_secs\":{ttl_secs}}}");
                    } else {
                        println!("Lease granted.");
                        println!("  ID:      {id}");
                        println!("  Tool:    {tool}");
                        println!("  Env var: {env_var}");
                        println!("  TTL:     {ttl} ({ttl_secs}s)");
                        println!();
                        println!("The env var will be injected when '{tool}' is called via MCP.");
                        println!("Run `raios secret revoke {id}` to revoke early.");
                    }
                }
                Err(e) => {
                    eprintln!("DB error: {e}");
                    std::process::exit(1);
                }
            }
        }
        List => {
            let leases = secret_lease::list_active(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&leases).unwrap_or_default());
                return;
            }
            if leases.is_empty() {
                println!("No active secret leases.");
                return;
            }
            println!("{:<20}  {:<25}  {:<20}  EXPIRES", "ID", "TOOL", "ENV_VAR");
            for l in &leases {
                println!(
                    "{:<20}  {:<25}  {:<20}  {}",
                    l.id, l.tool, l.env_var, l.expires_at
                );
            }
        }
        All => {
            let leases = secret_lease::list_all(&conn).unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&leases).unwrap_or_default());
                return;
            }
            if leases.is_empty() {
                println!("No secret leases found.");
                return;
            }
            println!(
                "{:<20}  {:<25}  {:<20}  {:<10}  EXPIRES",
                "ID", "TOOL", "ENV_VAR", "STATUS"
            );
            for l in &leases {
                println!(
                    "{:<20}  {:<25}  {:<20}  {:<10}  {}",
                    l.id, l.tool, l.env_var, l.status, l.expires_at
                );
            }
        }
        Revoke { id } => match secret_lease::revoke(&conn, &id) {
            Ok(true) => {
                if json {
                    println!("{{\"status\":\"revoked\",\"id\":\"{id}\"}}");
                } else {
                    println!("Lease {id} revoked.");
                }
            }
            Ok(false) => {
                eprintln!("No active lease with id '{id}'.");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("DB error: {e}");
                std::process::exit(1);
            }
        },
    }
}
