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
    "conflux-bevy",
    "wgpu",
    "residency-core",
];

/// Workspace crates other than the adapter must stay free of Bevy dependencies.
const BEVY_ADAPTER_CRATE: &str = "conflux-bevy";

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

fn collect_boundary_violations(packages: &[Value]) -> Vec<String> {
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

            // Bevy is an engine adapter concern only. No Conflux crate may grow a
            // direct or indirect engine API except the adapter crate itself.
            if is_bevy_dependency(dep_name) && name != BEVY_ADAPTER_CRATE {
                violations.push(format!(
                    "`{name}` depends on Bevy crate `{dep_name}` ({kind}); Bevy dependencies are allowed only in conflux-bevy"
                ));
            }

            // Other Conflux crates may not depend on the Bevy adapter either, or
            // they would import engine integration through the adapter boundary.
            if dep_name == BEVY_ADAPTER_CRATE && name != BEVY_ADAPTER_CRATE {
                violations.push(format!(
                    "`{name}` depends on `{dep_name}` ({kind}); Bevy adapter code is allowed only in conflux-bevy"
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

    violations
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

    let violations = collect_boundary_violations(packages);

    assert!(
        violations.is_empty(),
        "crate dependency-boundary violations:\n  - {}",
        violations.join("\n  - ")
    );
}

#[test]
fn bevy_dependencies_are_allowed_only_in_the_adapter() {
    let packages = vec![package(
        "conflux-runtime",
        &[dep("bevy_ecs", "normal", false)],
    )];

    let violations = collect_boundary_violations(&packages);

    assert!(violations.iter().any(|violation| {
        violation.contains("conflux-runtime")
            && violation.contains("bevy_ecs")
            && violation.contains("allowed only in conflux-bevy")
    }));
}

#[test]
fn bevy_dependencies_are_allowed_inside_the_adapter() {
    let packages = vec![package("conflux-bevy", &[dep("bevy_ecs", "normal", false)])];

    let violations = collect_boundary_violations(&packages);

    assert!(
        violations.is_empty(),
        "unexpected violations: {violations:?}"
    );
}

#[test]
fn other_conflux_crates_may_not_depend_on_the_bevy_adapter() {
    let packages = vec![package(
        "conflux-planner",
        &[dep("conflux-bevy", "normal", false)],
    )];

    let violations = collect_boundary_violations(&packages);

    assert!(violations.iter().any(|violation| {
        violation.contains("conflux-planner")
            && violation.contains("conflux-bevy")
            && violation.contains("allowed only in conflux-bevy")
    }));
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

fn is_bevy_dependency(dep_name: &str) -> bool {
    dep_name == "bevy" || dep_name.starts_with("bevy_")
}

fn package(name: &str, dependencies: &[Value]) -> Value {
    let dependencies = Value::Array(dependencies.to_vec());
    let mut package = serde_json::Map::new();
    package.insert("name".to_string(), Value::String(name.to_string()));
    package.insert("dependencies".to_string(), dependencies);
    Value::Object(package)
}

fn dep(name: &str, kind: &str, optional: bool) -> Value {
    let mut dependency = serde_json::Map::new();
    dependency.insert("name".to_string(), Value::String(name.to_string()));
    dependency.insert(
        "kind".to_string(),
        if kind == "normal" {
            Value::Null
        } else {
            Value::String(kind.to_string())
        },
    );
    dependency.insert("optional".to_string(), Value::Bool(optional));
    Value::Object(dependency)
}
