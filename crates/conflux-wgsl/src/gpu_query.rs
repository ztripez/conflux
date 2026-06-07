//! Exact bounded-radius proximity-query GPU scan (feature `gpu`).
//!
//! This module deliberately implements only the query shapes that can preserve the
//! existing proximity contract exactly with WGSL's integer arithmetic. It evaluates
//! a full source × target candidate matrix on the GPU, reads the exact eligibility
//! flags back, and then applies the canonical stable ordering on the CPU. That keeps
//! runtime/core free of hidden GPU dispatch while giving the GPU feature an opt-in
//! equivalence surface for proximity-query execution.

use wgpu::util::DeviceExt;

use conflux_ir::{
    finalize_query_neighbors, Grid2, QueryIr, QueryLimit, QueryMetric, QueryOrdering,
    QuerySourceResult, SelfPolicy,
};

use super::{
    create_compute_pipeline, create_storage_bind_group_layout, read_back_u32, staging_buffer,
};
use crate::gpu::{Access, GpuError, GpuExecutor};

const WORKGROUP_SIZE: u32 = 64;
const PARAM_WORDS: usize = 6;
const U32_SIZE: usize = std::mem::size_of::<u32>();

const SHADER: &str = r"
struct Positions { values: array<u32>, };
struct Flags { values: array<u32>, };
struct Params { values: array<u32, 6>, };

@group(0) @binding(0) var<storage, read> source_positions: Positions;
@group(0) @binding(1) var<storage, read> target_positions: Positions;
@group(0) @binding(2) var<storage, read> params: Params;
@group(0) @binding(3) var<storage, read_write> flags: Flags;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let pair = id.x;
    let source_count = params.values[0];
    let target_count = params.values[1];
    let width = params.values[2];
    let radius = params.values[3];
    let metric = params.values[4];
    let exclude_same_set_self = params.values[5];
    let pair_count = source_count * target_count;
    if (pair >= pair_count) {
        return;
    }

    let source_actor = pair / target_count;
    let target_actor = pair % target_count;
    if (exclude_same_set_self == 1u && source_actor == target_actor) {
        flags.values[pair] = 0u;
        return;
    }

    let source_cell = source_positions.values[source_actor];
    let target_cell = target_positions.values[target_actor];
    let sx = source_cell % width;
    let sy = source_cell / width;
    let tx = target_cell % width;
    let ty = target_cell / width;
    let dx = max(sx, tx) - min(sx, tx);
    let dy = max(sy, ty) - min(sy, ty);
    let distance = select(max(dx, dy), dx + dy, metric == 1u);
    flags.values[pair] = select(0u, 1u, distance <= radius);
}
";

/// The concrete path used by the proximity GPU helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProximityGpuExecutionPath {
    /// Full exact source × target scan on the GPU, followed by stable ordering of
    /// exact distances after readback.
    ExactGpuScan,
}

/// Dispatch and readback metadata for one proximity-query GPU run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProximityGpuRunMetadata {
    /// Concrete GPU query path that executed.
    pub path: ProximityGpuExecutionPath,
    /// Number of source actors evaluated.
    pub source_count: usize,
    /// Number of target actors considered for each source.
    pub target_count: usize,
    /// Number of source-target pairs evaluated by the shader.
    pub pair_count: usize,
    /// Number of compute workgroups submitted in the x dimension.
    pub dispatched_workgroups: u32,
    /// Bytes copied back for the candidate-flag matrix.
    pub flag_bytes: u64,
}

/// Result of one exact proximity-query GPU scan.
#[derive(Clone, Debug, PartialEq)]
pub struct ProximityGpuRun {
    /// Query name from the lowered IR.
    pub query: String,
    /// Dispatch/readback metadata that distinguishes this from the CPU scan/index.
    pub metadata: ProximityGpuRunMetadata,
    /// One result per source actor, in source-actor index order.
    pub sources: Vec<QuerySourceResult>,
}

impl GpuExecutor {
    /// Executes one exact bounded-radius proximity query on this executor's GPU.
    ///
    /// This phase accepts only [`QueryLimit::Within`] queries using Chebyshev or
    /// Manhattan metrics. Those distances are integral on the row-major grid, so
    /// WGSL can classify candidate pairs exactly by comparing against
    /// `floor(radius)`. [`QueryLimit::KNearest`] and Euclidean radius queries are
    /// refused visibly instead of approximated.
    ///
    /// # Errors
    ///
    /// Returns [`GpuError::UnsupportedProximityQuery`] for query shapes that do not
    /// have an exact GPU strategy in this phase, [`GpuError::InvalidActorPosition`]
    /// for out-of-grid actor positions, [`GpuError::ProximityDispatchSizeOverflow`]
    /// for source × target matrices that exceed the executor's dispatch shape,
    /// [`GpuError::InvalidProximityReadback`] or [`GpuError::InvalidProximityFlag`]
    /// for malformed shader output, or shader/device/readback errors from wgpu.
    pub fn run_proximity_query(
        &self,
        query: &QueryIr,
        grid: Grid2,
        source_set: &str,
        source_positions: &[usize],
        target_set: &str,
        target_positions: &[usize],
    ) -> Result<ProximityGpuRun, GpuError> {
        let plan = validate_proximity_query(
            query,
            grid,
            source_set,
            source_positions,
            target_set,
            target_positions,
        )?;
        if plan.pair_count == 0 {
            return Ok(ProximityGpuRun {
                query: query.name.clone(),
                metadata: plan.metadata(),
                sources: finalize_sources(query, grid, source_positions, target_positions, &[])?,
            });
        }

        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("conflux-proximity-query"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        self.pop_shader_error_scope()?;

        let source_gpu = u32_buffer(&self.device, "source positions", &plan.source_positions);
        let target_gpu = u32_buffer(&self.device, "target positions", &plan.target_positions);
        let params_gpu = u32_buffer(&self.device, "proximity params", &plan.params);
        let flags_gpu = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("proximity flags"),
            size: plan.flag_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let flags_staging = staging_buffer(&self.device, plan.flag_bytes);

        let layout = create_storage_bind_group_layout(
            &self.device,
            [
                (0, Access::Read),
                (1, Access::Read),
                (2, Access::Read),
                (3, Access::ReadWrite),
            ],
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("proximity bind group"),
            layout: &layout,
            entries: &[
                bind_entry(0, &source_gpu),
                bind_entry(1, &target_gpu),
                bind_entry(2, &params_gpu),
                bind_entry(3, &flags_gpu),
            ],
        });
        let pipeline = create_compute_pipeline(self, &shader, &layout, "main")?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("proximity scan"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(plan.dispatched_workgroups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&flags_gpu, 0, &flags_staging, 0, plan.flag_bytes);
        self.queue.submit(Some(encoder.finish()));
        let flags = read_back_u32(&self.device, &flags_staging)?;

        Ok(ProximityGpuRun {
            query: query.name.clone(),
            metadata: plan.metadata(),
            sources: finalize_sources(query, grid, source_positions, target_positions, &flags)?,
        })
    }
}

/// Executes one exact bounded-radius proximity query on a newly acquired GPU.
///
/// Returns `Ok(None)` when no adapter is available so optional hardware checks can
/// skip without failing default CPU-only environments. Query shape and input
/// validation runs before adapter lookup, so unsupported or invalid requests are
/// still refused visibly on CPU-only hosts.
///
/// # Errors
///
/// Returns [`GpuError`] for unsupported query shapes, invalid positions, dispatch
/// overflow, malformed readback flags, device creation failures, shader
/// validation, or readback failures.
pub fn run_proximity_query_on_gpu(
    query: &QueryIr,
    grid: Grid2,
    source_set: &str,
    source_positions: &[usize],
    target_set: &str,
    target_positions: &[usize],
) -> Result<Option<ProximityGpuRun>, GpuError> {
    validate_proximity_query(
        query,
        grid,
        source_set,
        source_positions,
        target_set,
        target_positions,
    )?;
    let Some(executor) = GpuExecutor::new()? else {
        return Ok(None);
    };
    executor
        .run_proximity_query(
            query,
            grid,
            source_set,
            source_positions,
            target_set,
            target_positions,
        )
        .map(Some)
}

#[derive(Clone, Debug)]
struct ProximityPlan {
    source_positions: Vec<u32>,
    target_positions: Vec<u32>,
    params: [u32; PARAM_WORDS],
    pair_count: usize,
    dispatched_workgroups: u32,
    flag_bytes: u64,
}

impl ProximityPlan {
    fn metadata(&self) -> ProximityGpuRunMetadata {
        ProximityGpuRunMetadata {
            path: ProximityGpuExecutionPath::ExactGpuScan,
            source_count: self.source_positions.len(),
            target_count: self.target_positions.len(),
            pair_count: self.pair_count,
            dispatched_workgroups: self.dispatched_workgroups,
            flag_bytes: self.flag_bytes,
        }
    }
}

fn validate_proximity_query(
    query: &QueryIr,
    grid: Grid2,
    source_set: &str,
    source_positions: &[usize],
    target_set: &str,
    target_positions: &[usize],
) -> Result<ProximityPlan, GpuError> {
    if query.ordering != QueryOrdering::DistanceThenIndex {
        return Err(GpuError::UnsupportedProximityQuery {
            query: query.name.clone(),
            reason: "only DistanceThenIndex ordering is implemented exactly".to_string(),
        });
    }
    let QueryLimit::Within(radius) = query.limit else {
        return Err(GpuError::UnsupportedProximityQuery {
            query: query.name.clone(),
            reason: "KNearest needs an exact expanding-ring GPU strategy and is deferred"
                .to_string(),
        });
    };
    if query.metric == QueryMetric::Euclidean {
        return Err(GpuError::UnsupportedProximityQuery {
            query: query.name.clone(),
            reason: "Euclidean radius comparisons need an exact f64/squared-distance strategy before GPU execution".to_string(),
        });
    }
    if !radius.is_finite() || radius < 0.0 {
        return Err(GpuError::UnsupportedProximityQuery {
            query: query.name.clone(),
            reason: "radius must be finite and non-negative".to_string(),
        });
    }
    let radius_floor = radius.floor();
    if radius_floor > f64::from(u32::MAX) {
        return Err(GpuError::UnsupportedProximityQuery {
            query: query.name.clone(),
            reason: "radius exceeds the u32 grid-distance range supported by WGSL".to_string(),
        });
    }

    let source_positions = validate_positions(source_set, source_positions, grid)?;
    let target_positions = validate_positions(target_set, target_positions, grid)?;
    let source_count = u32::try_from(source_positions.len()).map_err(|_| {
        GpuError::ProximityDispatchSizeOverflow {
            source_count: source_positions.len(),
            target_count: target_positions.len(),
        }
    })?;
    let target_count = u32::try_from(target_positions.len()).map_err(|_| {
        GpuError::ProximityDispatchSizeOverflow {
            source_count: source_positions.len(),
            target_count: target_positions.len(),
        }
    })?;
    let width = u32::try_from(grid.width).map_err(|_| GpuError::UnsupportedProximityQuery {
        query: query.name.clone(),
        reason: "grid width exceeds the u32 range supported by WGSL".to_string(),
    })?;
    let pair_count = source_positions
        .len()
        .checked_mul(target_positions.len())
        .ok_or(GpuError::ProximityDispatchSizeOverflow {
            source_count: source_positions.len(),
            target_count: target_positions.len(),
        })?;
    let pair_count_u32 =
        u32::try_from(pair_count).map_err(|_| GpuError::ProximityDispatchSizeOverflow {
            source_count: source_positions.len(),
            target_count: target_positions.len(),
        })?;
    let flag_bytes = u32_byte_len(pair_count).ok_or(GpuError::ProximityDispatchSizeOverflow {
        source_count: source_positions.len(),
        target_count: target_positions.len(),
    })?;
    let dispatched_workgroups = if pair_count_u32 == 0 {
        0
    } else {
        pair_count_u32.div_ceil(WORKGROUP_SIZE)
    };

    Ok(ProximityPlan {
        source_positions,
        target_positions,
        params: [
            source_count,
            target_count,
            width,
            radius_floor as u32,
            metric_code(query.metric),
            u32::from(query.source == query.target && query.self_policy == SelfPolicy::Exclude),
        ],
        pair_count,
        dispatched_workgroups,
        flag_bytes,
    })
}

fn validate_positions(set: &str, positions: &[usize], grid: Grid2) -> Result<Vec<u32>, GpuError> {
    let cells = grid.cells();
    positions
        .iter()
        .enumerate()
        .map(|(actor, &cell)| {
            if cell >= cells {
                return Err(GpuError::InvalidActorPosition {
                    set: set.to_string(),
                    actor,
                    cell,
                    cells,
                });
            }
            u32::try_from(cell).map_err(|_| GpuError::InvalidActorPosition {
                set: set.to_string(),
                actor,
                cell,
                cells,
            })
        })
        .collect()
}

fn metric_code(metric: QueryMetric) -> u32 {
    match metric {
        QueryMetric::Chebyshev => 0,
        QueryMetric::Manhattan => 1,
        QueryMetric::Euclidean => unreachable!("Euclidean queries are rejected before encoding"),
    }
}

fn finalize_sources(
    query: &QueryIr,
    grid: Grid2,
    source_positions: &[usize],
    target_positions: &[usize],
    flags: &[u32],
) -> Result<Vec<QuerySourceResult>, GpuError> {
    let expected = source_positions
        .len()
        .checked_mul(target_positions.len())
        .ok_or(GpuError::ProximityDispatchSizeOverflow {
            source_count: source_positions.len(),
            target_count: target_positions.len(),
        })?;
    if flags.len() != expected {
        return Err(GpuError::InvalidProximityReadback {
            actual: flags.len(),
            expected,
        });
    }

    source_positions
        .iter()
        .enumerate()
        .map(|(source_actor, &source_cell)| {
            let mut candidate_targets = Vec::new();
            for target_actor in 0..target_positions.len() {
                let flag_index = source_actor * target_positions.len() + target_actor;
                match flags[flag_index] {
                    0 => {}
                    1 => candidate_targets.push(target_actor),
                    flag => {
                        return Err(GpuError::InvalidProximityFlag {
                            source_actor,
                            target_actor,
                            flag,
                        });
                    }
                }
            }
            let neighbors = finalize_query_neighbors(
                source_actor,
                source_cell,
                target_positions,
                &candidate_targets,
                query,
                grid,
                query.source == query.target,
            );
            Ok(QuerySourceResult {
                source_actor,
                neighbors,
            })
        })
        .collect()
}

fn u32_byte_len(values: usize) -> Option<u64> {
    values
        .checked_mul(U32_SIZE)
        .and_then(|bytes| u64::try_from(bytes).ok())
}

fn u32_buffer(device: &wgpu::Device, label: &str, values: &[u32]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(values),
        usage: wgpu::BufferUsages::STORAGE,
    })
}

fn bind_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(limit: QueryLimit, metric: QueryMetric) -> QueryIr {
        QueryIr {
            name: "nearby".to_string(),
            source: 0,
            target: 0,
            metric,
            limit,
            self_policy: SelfPolicy::Exclude,
            ordering: QueryOrdering::DistanceThenIndex,
            approximation: conflux_ir::ApproximationPolicy::Exact,
        }
    }

    #[test]
    fn validates_exact_manhattan_radius_plan_without_gpu() {
        let plan = validate_proximity_query(
            &query(QueryLimit::Within(2.9), QueryMetric::Manhattan),
            Grid2::new(4, 4),
            "herd",
            &[0, 5],
            "herd",
            &[0, 1, 10],
        )
        .expect("bounded integer-metric query should be GPU eligible");

        assert_eq!(plan.params, [2, 3, 4, 2, 1, 1]);
        assert_eq!(plan.pair_count, 6);
        assert_eq!(plan.dispatched_workgroups, 1);
        assert_eq!(plan.flag_bytes, 24);
    }

    #[test]
    fn rejects_k_nearest_without_gpu() {
        let err = validate_proximity_query(
            &query(QueryLimit::KNearest(2), QueryMetric::Manhattan),
            Grid2::new(4, 4),
            "herd",
            &[0],
            "herd",
            &[1],
        )
        .expect_err("k-nearest must not silently approximate on GPU");

        assert!(matches!(err, GpuError::UnsupportedProximityQuery { .. }));
    }

    #[test]
    fn rejects_euclidean_radius_without_gpu() {
        let err = validate_proximity_query(
            &query(QueryLimit::Within(2.0), QueryMetric::Euclidean),
            Grid2::new(4, 4),
            "herd",
            &[0],
            "herd",
            &[1],
        )
        .expect_err("Euclidean GPU radius must wait for an exact strategy");

        assert!(matches!(err, GpuError::UnsupportedProximityQuery { .. }));
    }

    #[test]
    fn rejects_out_of_grid_position_without_gpu() {
        let err = validate_proximity_query(
            &query(QueryLimit::Within(2.0), QueryMetric::Chebyshev),
            Grid2::new(2, 2),
            "herd",
            &[4],
            "herd",
            &[1],
        )
        .expect_err("invalid positions must fail before adapter lookup");

        assert!(matches!(
            err,
            GpuError::InvalidActorPosition {
                actor: 0,
                cell: 4,
                ..
            }
        ));
    }

    #[test]
    fn public_helper_rejects_invalid_query_before_adapter_lookup() {
        let err = run_proximity_query_on_gpu(
            &query(QueryLimit::KNearest(2), QueryMetric::Manhattan),
            Grid2::new(4, 4),
            "herd",
            &[0],
            "herd",
            &[1],
        )
        .expect_err("unsupported queries must not become no-adapter skips");

        assert!(matches!(err, GpuError::UnsupportedProximityQuery { .. }));
    }

    #[test]
    fn finalization_keeps_same_set_self_policy_canonical() {
        let run = finalize_sources(
            &query(QueryLimit::Within(3.0), QueryMetric::Manhattan),
            Grid2::new(4, 4),
            &[5],
            &[5],
            &[1],
        )
        .expect("well-shaped flags should finalize");

        assert_eq!(run[0].source_actor, 0);
        assert!(run[0].neighbors.is_empty());
    }

    #[test]
    fn finalizes_flags_to_stable_distance_then_index_order() {
        let mut cross_set_query = query(QueryLimit::Within(3.0), QueryMetric::Manhattan);
        cross_set_query.target = 1;
        let run = finalize_sources(
            &cross_set_query,
            Grid2::new(4, 4),
            &[5],
            &[6, 1, 9],
            &[1, 1, 1],
        )
        .expect("well-shaped flags should finalize");

        assert_eq!(run[0].source_actor, 0);
        assert_eq!(
            run[0].neighbors,
            vec![
                conflux_ir::QueryNeighbor {
                    target_actor: 0,
                    distance: 1.0,
                },
                conflux_ir::QueryNeighbor {
                    target_actor: 1,
                    distance: 1.0,
                },
                conflux_ir::QueryNeighbor {
                    target_actor: 2,
                    distance: 1.0,
                },
            ]
        );
    }

    #[test]
    fn rejects_malformed_flag_readback_without_gpu() {
        let err = finalize_sources(
            &query(QueryLimit::Within(3.0), QueryMetric::Manhattan),
            Grid2::new(4, 4),
            &[5],
            &[6, 1],
            &[1],
        )
        .expect_err("malformed readback must not be treated as no-neighbor flags");

        assert!(matches!(
            err,
            GpuError::InvalidProximityReadback {
                actual: 1,
                expected: 2
            }
        ));
    }

    #[test]
    fn rejects_invalid_flag_value_without_gpu() {
        let err = finalize_sources(
            &query(QueryLimit::Within(3.0), QueryMetric::Manhattan),
            Grid2::new(4, 4),
            &[5],
            &[6],
            &[2],
        )
        .expect_err("invalid readback flags must not be treated as neighbors");

        assert!(matches!(
            err,
            GpuError::InvalidProximityFlag {
                source_actor: 0,
                target_actor: 0,
                flag: 2,
            }
        ));
    }
}
