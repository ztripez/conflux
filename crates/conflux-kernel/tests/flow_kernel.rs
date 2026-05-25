//! Flow-kernel extraction + optimized CPU execution.

use conflux_core::{cell, field_lit, lower, neighbor, EdgePolicy, Field, Flow, Grid2, Model};
use conflux_kernel::{
    execute_flow, extract_flows, FlowKernelDestination, FlowRejectionReason, ScalarType,
};

/// A 3x1 `Terrain` with a `water` stock = [8, 0, 4], lowered with `flow` added.
fn flow_model(flow: Flow) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(3, 1));
    terrain.stock("water", vec![8.0, 0.0, 4.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(flow);
    model
}

fn runoff() -> Flow {
    Flow::new("runoff")
        .on_field("Terrain")
        .move_channel("water")
        .amount(cell("water") * field_lit(0.5))
        .to_neighbor(1, 0, EdgePolicy::Reject)
        .conserved()
}

#[test]
fn extracts_a_fixed_offset_flow_kernel() {
    let ir = lower(&flow_model(runoff())).unwrap();
    let report = extract_flows(&ir);

    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.rejected_count(), 0);
    let kernel = &report.accepted[0];
    assert_eq!(kernel.name, "runoff");
    assert_eq!(kernel.field_name, "Terrain");
    assert_eq!(kernel.channel_name, "water");
    assert_eq!((kernel.dx, kernel.dy), (1, 0));
    assert_eq!(kernel.edge, EdgePolicy::Reject);
    assert_eq!(kernel.scalar_type, ScalarType::F32);
    // The amount reads only the current cell, so the stencil radius is 0.
    assert_eq!(kernel.stencil_radius, 0);
    assert_eq!(kernel.amount_channels.len(), 1);
    assert_eq!(kernel.amount_channels[0].name, "water");
}

#[test]
fn executes_the_flow_scatter_with_boundary_loss() {
    // water = [8, 0, 4], amount = water * 0.5, move east, Reject edge:
    // cell 0 -> cell 1 (amount 4); cell 1 emits 0 (skipped); cell 2 -> off-grid
    // boundary (amount 2). Result: [8-4, 0+4, 4-2] = [4, 4, 2], boundary loss 2.
    let ir = lower(&flow_model(runoff())).unwrap();
    let kernel = &extract_flows(&ir).accepted[0];

    let out = execute_flow(kernel, &[vec![8.0, 0.0, 4.0]]);
    assert_eq!(out.channel, vec![4.0, 4.0, 2.0]);
    assert_eq!(out.boundary_loss, 2.0);
    assert_eq!(out.transfers.len(), 2);
    assert_eq!(out.transfers[0].source, 0);
    assert_eq!(out.transfers[0].destination, FlowKernelDestination::Cell(1));
    assert_eq!(out.transfers[0].amount, 4.0);
    assert_eq!(out.transfers[1].source, 2);
    assert_eq!(
        out.transfers[1].destination,
        FlowKernelDestination::Boundary
    );
    assert_eq!(out.transfers[1].amount, 2.0);
}

#[test]
fn the_amount_is_computed_in_f32() {
    // 1x1 grid, water = [0.1] (not exactly representable in f32), amount = water,
    // destination off-grid (boundary). The emitted amount is the f32-rounded value,
    // proving the optimized path computes the amount in f32 like the other kernels.
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("water", vec![0.1]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("drain")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water"))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    let ir = lower(&model).unwrap();
    let kernel = &extract_flows(&ir).accepted[0];

    let out = execute_flow(kernel, &[vec![0.1]]);
    let f32_rounded = 0.1f32 as f64;
    assert_eq!(out.transfers[0].amount, f32_rounded);
    assert_ne!(out.transfers[0].amount, 0.1f64, "computed in f32, not f64");
    assert_eq!(out.boundary_loss, f32_rounded);
}

#[test]
fn a_wrap_destination_credits_a_wrapped_cell_with_no_boundary_loss() {
    // water = [8, 0, 4], move east with Wrap: cell 2 wraps to cell 0 instead of
    // leaving the grid, so the total is conserved (no boundary loss).
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Wrap)
            .conserved(),
    ))
    .unwrap();
    let kernel = &extract_flows(&ir).accepted[0];

    let out = execute_flow(kernel, &[vec![8.0, 0.0, 4.0]]);
    // cell 0 -> cell 1 (4); cell 2 -> cell 0 wrapped (2). [8-4+2, 0+4, 4-2].
    assert_eq!(out.channel, vec![6.0, 4.0, 2.0]);
    assert_eq!(out.boundary_loss, 0.0);
    assert!(out
        .transfers
        .iter()
        .all(|t| !matches!(t.destination, FlowKernelDestination::Boundary)));
}

#[test]
fn an_amount_reading_a_within_radius_neighbor_is_accepted() {
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(neighbor("water", 1, 0, EdgePolicy::Wrap) * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    ))
    .unwrap();
    let report = extract_flows(&ir);
    assert_eq!(report.accepted_count(), 1);
    assert_eq!(report.accepted[0].stencil_radius, 1);
}

#[test]
fn the_amount_binds_the_correct_absolute_channel_index() {
    // A two-channel field: amount = water * rate reads both, so the amount must bind
    // `rate` at its absolute index (1), not channel 0.
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain
        .stock("water", vec![10.0, 0.0])
        .signal("rate", vec![0.5, 0.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * cell("rate"))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    );
    let ir = lower(&model).unwrap();
    let kernel = &extract_flows(&ir).accepted[0];

    // channels addressed by absolute index: [0]=water, [1]=rate.
    let out = execute_flow(kernel, &[vec![10.0, 0.0], vec![0.5, 0.0]]);
    // cell 0 emits 10 * 0.5 = 5 -> cell 1; cell 1 emits 0.
    assert_eq!(out.channel, vec![5.0, 5.0]);
}

#[test]
fn overdraw_is_reported_as_a_negative_source_not_clamped() {
    // amount (10) exceeds the source (1): the source goes negative — instability is
    // reported as data, never clamped to available source.
    let mut terrain = Field::new("Terrain", Grid2::new(1, 1));
    terrain.stock("water", vec![1.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_flow(
        Flow::new("drain")
            .on_field("Terrain")
            .move_channel("water")
            .amount(cell("water") * field_lit(10.0))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .boundary_loss(),
    );
    let ir = lower(&model).unwrap();
    let kernel = &extract_flows(&ir).accepted[0];

    let out = execute_flow(kernel, &[vec![1.0]]);
    assert_eq!(out.channel, vec![-9.0]); // 1 - 10, not clamped to 0
    assert_eq!(out.boundary_loss, 10.0);
}

#[test]
fn an_over_wide_amount_stencil_is_rejected() {
    let ir = lower(&flow_model(
        Flow::new("runoff")
            .on_field("Terrain")
            .move_channel("water")
            .amount(neighbor("water", 2, 0, EdgePolicy::Wrap) * field_lit(0.5))
            .to_neighbor(1, 0, EdgePolicy::Reject)
            .conserved(),
    ))
    .unwrap();
    let report = extract_flows(&ir);

    assert_eq!(report.accepted_count(), 0);
    assert_eq!(report.rejected_count(), 1);
    assert_eq!(report.rejected[0].flow, "runoff");
    match &report.rejected[0].reason {
        FlowRejectionReason::AmountStencilTooWide { dx, dy, .. } => {
            assert_eq!((*dx, *dy), (2, 0));
        }
    }
}

#[test]
fn models_without_flows_extract_no_flow_kernels() {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 1));
    terrain.stock("water", vec![1.0, 1.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    let ir = lower(&model).unwrap();
    let report = extract_flows(&ir);
    assert_eq!(report.accepted_count(), 0);
    assert_eq!(report.rejected_count(), 0);
}
