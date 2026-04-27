use std::{fs, path::PathBuf};

const BANNED_ROUTE_SOURCE_FRAGMENTS: &[&str] = &[
    "api-v3.raydium.io",
    "api.raydium.io",
    "api-v3.raydium.io/pools/info/mint",
    "launch-mint-v1.raydium.io/main/configs",
    "launch-mint-v1-devnet.raydium.io/main/configs",
    "dexscreener.com",
    "quote-api.jup.ag",
    "pumpportal.fun",
    "rpc-damm-v2-scan",
    "damm-pools-by-pair",
    "rpc-pool-scan",
    "verified-damm-pair",
];

const CANONICAL_ROUTE_MODULES: &[&str] = &[
    "rust/execution-engine/src/pump_native.rs",
    "rust/execution-engine/src/raydium_amm_v4_native.rs",
    "rust/execution-engine/src/bonk_execution_support.rs",
    "rust/launchdeck-engine/src/bonk_native.rs",
    "rust/execution-engine/src/bags_execution_support.rs",
    "rust/launchdeck-engine/src/bags_native.rs",
];

// Allowed external sources are documented in docs/internal/ROUTE_SOURCE_POLICY.md.
// This guard is intentionally scoped to execution route/config modules so
// VAMP, metadata, fee feeds, transport providers, and Bags launch/setup flows
// remain untouched.
#[test]
fn canonical_route_modules_do_not_use_banned_external_sources() {
    let repo_root = repo_root();
    let mut violations = Vec::new();
    for relative_path in CANONICAL_ROUTE_MODULES {
        let path = repo_root.join(relative_path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for fragment in BANNED_ROUTE_SOURCE_FRAGMENTS {
            if source.contains(fragment) {
                violations.push(format!("{relative_path} contains {fragment}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "banned route/config sources found:\n{}",
        violations.join("\n")
    );
}

#[test]
fn raydium_v4_routes_are_explicit_pool_only() {
    let repo_root = repo_root();
    let pump = fs::read_to_string(repo_root.join("rust/execution-engine/src/pump_native.rs"))
        .expect("pump native source");
    assert!(
        !pump.contains("PumpRaydiumAmm")
            && !pump.contains("raydium_amm_v4")
            && !pump.contains("Raydium AMM v4"),
        "Pump native must not own Raydium AMM v4 discovery or execution"
    );

    let raydium =
        fs::read_to_string(repo_root.join("rust/execution-engine/src/raydium_amm_v4_native.rs"))
            .expect("raydium amm v4 native source");
    assert!(
        raydium.contains("routing requires an explicit pool address"),
        "Raydium AMM v4 must require explicit pool-address routing"
    );

    let bonk =
        fs::read_to_string(repo_root.join("rust/execution-engine/src/bonk_execution_support.rs"))
            .expect("bonk execution support source");
    assert!(
        bonk.contains("launchpad_candidates")
            && bonk.contains("candidate.quote_asset == quote.asset && candidate.complete"),
        "Bonk migrated Raydium must be anchored to a matching complete launchpad candidate"
    );
}

#[test]
fn route_source_policy_documents_allowed_integrations() {
    let policy = fs::read_to_string(repo_root().join("docs/internal/ROUTE_SOURCE_POLICY.md"))
        .expect("route source policy doc");
    for required in [
        "Bags public API",
        "Fee feeds",
        "Explicit Raydium AMM v4 pool inputs",
        "VAMP/import enrichment",
        "Trusted stable swaps",
    ] {
        assert!(
            policy.contains(required),
            "route source policy is missing allowed category: {required}"
        );
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("repo root")
        .to_path_buf()
}
