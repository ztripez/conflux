//! Mechanical dependency-boundary guard.
//!
//! Reads the workspace's declared dependencies via `cargo metadata --no-deps`
//! (offline; it only parses manifests) and fails if any crate violates the
//! boundary rules from `docs/BOUNDARIES.md`. Each violation names the offending
//! crate and dependency. This is the deterministic enforcement of rules that were
//! previously convention-only.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// Crates that must stay free of backend / Residency / GPU dependencies.
const CORE_CRATES: &[&str] = &[
    "conflux-core",
    "conflux-ir",
    "conflux-kernel",
    "conflux-runtime",
];

/// What a core crate may never depend on (in any dependency kind).
const FORBIDDEN_IN_CORE: &[&str] = &[
    "conflux-residency",
    "conflux-wgsl",
    "conflux-planner",
    "conflux-trace",
    "wgpu",
    "residency-core",
];

fn workspace_manifest() -> PathBuf {
    // CARGO_MANIFEST_DIR is crates/conflux-arch-guard; the workspace root is two
    // levels up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate lives at crates/<name>")
        .join("Cargo.toml")
}

fn metadata() -> Value {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .arg("--manifest-path")
        .arg(workspace_manifest())
        .output()
        .expect("failed to run `cargo metadata`");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata emits valid JSON")
}

/// The dependency kind: `null` for a normal dependency, otherwise `"dev"` /
/// `"build"`.
fn kind_label(dep: &Value) -> &str {
    match dep.get("kind") {
        Some(Value::String(s)) => s,
        _ => "normal",
    }
}

#[test]
fn crate_dependency_boundaries_hold() {
    let metadata = metadata();
    let packages = metadata["packages"]
        .as_array()
        .expect("metadata has a packages array");

    let mut violations: Vec<String> = Vec::new();

    for pkg in packages {
        let name = pkg["name"].as_str().expect("package has a name");
        let deps = pkg["dependencies"].as_array().cloned().unwrap_or_default();

        for dep in &deps {
            let dep_name = dep["name"].as_str().expect("dependency has a name");
            let optional = dep["optional"].as_bool().unwrap_or(false);
            let kind = kind_label(dep);

            // residency-core may appear only in conflux-residency.
            if dep_name == "residency-core" && name != "conflux-residency" {
                violations.push(format!(
                    "`{name}` depends on `residency-core` ({kind}); allowed only in conflux-residency"
                ));
            }

            // wgpu may appear only in conflux-wgsl, optional and behind `gpu`.
            if dep_name == "wgpu" {
                if name != "conflux-wgsl" {
                    violations.push(format!(
                        "`{name}` depends on `wgpu` ({kind}); allowed only in conflux-wgsl"
                    ));
                } else {
                    if !optional {
                        violations.push(
                            "conflux-wgsl depends on `wgpu` non-optionally; it must be optional behind the `gpu` feature".to_string(),
                        );
                    }
                    if !gpu_feature_gates_wgpu(pkg) {
                        violations.push(
                            "conflux-wgsl's `wgpu` is not gated behind the `gpu` feature (`gpu = [\"dep:wgpu\", ...]`)".to_string(),
                        );
                    }
                }
            }

            // Core crates must depend on none of the backend/Residency/GPU crates.
            if CORE_CRATES.contains(&name) && FORBIDDEN_IN_CORE.contains(&dep_name) {
                violations.push(format!(
                    "core crate `{name}` depends on `{dep_name}` ({kind}); core crates must stay free of backend/Residency/GPU deps"
                ));
            }

            // conflux-trace may depend on other Conflux crates only as
            // dev-dependencies (any non-dev kind — normal or build — is a drift).
            if name == "conflux-trace" && kind != "dev" && dep_name.starts_with("conflux-") {
                violations.push(format!(
                    "conflux-trace has a normal dependency on `{dep_name}`; Conflux crate deps are allowed only as dev-dependencies"
                ));
            }

            // conflux-planner may read backend report crates, but never depend
            // directly on wgpu or residency-core.
            if name == "conflux-planner" && (dep_name == "wgpu" || dep_name == "residency-core") {
                violations.push(format!(
                    "conflux-planner depends directly on `{dep_name}` ({kind}); it may read backend report crates but not wgpu/residency-core directly"
                ));
            }

            // conflux-fixtures is test support: it may be a dev-dependency only,
            // so it can never become a hidden production API.
            if dep_name == "conflux-fixtures" && kind != "dev" {
                violations.push(format!(
                    "`{name}` has a {kind} dependency on `conflux-fixtures`; fixtures are test support and may be a dev-dependency only"
                ));
            }

            // conflux-fixtures itself keeps only conflux-core as a normal
            // dependency; everything else (the report crates) is dev-only, so the
            // fixtures' own surface stays minimal.
            if name == "conflux-fixtures" && kind == "normal" && dep_name != "conflux-core" {
                violations.push(format!(
                    "conflux-fixtures has a normal dependency on `{dep_name}`; it must keep only conflux-core as a normal dependency (other Conflux crates are dev-only)"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "crate dependency-boundary violations:\n  - {}",
        violations.join("\n  - ")
    );
}

/// True when the package's `gpu` feature enables the optional `wgpu` dependency
/// (`gpu = ["dep:wgpu", ...]`). This matches the explicit `dep:` form the manifest
/// uses; the implicit-feature form (an optional dep never referenced via `dep:`)
/// is intentionally not accepted, since it would not gate `wgpu` behind `gpu`.
fn gpu_feature_gates_wgpu(pkg: &Value) -> bool {
    pkg.get("features")
        .and_then(|f| f.get("gpu"))
        .and_then(|g| g.as_array())
        .is_some_and(|enables| {
            enables
                .iter()
                .filter_map(|v| v.as_str())
                .any(|s| s == "dep:wgpu")
        })
}
