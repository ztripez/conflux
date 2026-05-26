//! Advisory aggregate-optimization eligibility report.

use conflux_core::{lower, Aggregate, Field, Grid2, Model, Region};
use conflux_ir::AggregateOp;
use conflux_planner::{aggregate_eligibility, plan, AggregateCandidateShape};

fn aggregate_model(region: Region, aggregates: Vec<Aggregate>) -> Model {
    let mut terrain = Field::new("Terrain", Grid2::new(2, 2));
    terrain.stock("height", vec![1.0, 2.0, 3.0, 4.0]);
    let mut model = Model::new("world");
    model.add_field(terrain);
    model.add_region(region);
    for aggregate in aggregates {
        model.add_aggregate(aggregate);
    }
    model
}

#[test]
fn boolean_region_aggregate_is_precomputed_selection_eligible() {
    let ir = lower(&aggregate_model(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
        vec![Aggregate::sum("north_height", "north", "height")],
    ))
    .unwrap();
    let report = aggregate_eligibility(&ir);

    assert_eq!(report.aggregates.len(), 1);
    let aggregate = &report.aggregates[0];
    assert_eq!(aggregate.aggregate, "north_height");
    assert_eq!(aggregate.region, "north");
    assert_eq!(aggregate.field, "Terrain");
    assert_eq!(aggregate.channel.as_deref(), Some("height"));
    assert_eq!(aggregate.operation, AggregateOp::Sum);
    assert_eq!(aggregate.mask_kind, "boolean");
    assert_eq!(aggregate.selected_cells, 2);
    assert_eq!(aggregate.weight_total, 2.0);
    assert_eq!(aggregate.grid, (2, 2));
    assert!(aggregate.exact_reference_available);
    assert!(aggregate.eligible);
    assert_eq!(
        aggregate.candidate_shape,
        AggregateCandidateShape::PrecomputedRegionSelection
    );
    assert!(aggregate.rejections.is_empty());
    assert_eq!(report.eligible_count(), 1);
}

#[test]
fn weighted_region_aggregate_preserves_weight_summary() {
    let ir = lower(&aggregate_model(
        Region::new("weighted")
            .on_field("Terrain")
            .weights(vec![0.0, 0.5, 1.0, 0.25]),
        vec![Aggregate::mean("weighted_height", "weighted", "height")],
    ))
    .unwrap();
    let aggregate = &aggregate_eligibility(&ir).aggregates[0];

    assert_eq!(aggregate.mask_kind, "weighted");
    assert_eq!(aggregate.selected_cells, 3);
    assert_eq!(aggregate.weight_total, 1.75);
    assert!(aggregate.eligible);
}

#[test]
fn count_aggregate_has_no_channel_but_same_candidate_shape() {
    let ir = lower(&aggregate_model(
        Region::new("all")
            .on_field("Terrain")
            .mask(vec![true, true, true, true]),
        vec![Aggregate::count("cells", "all")],
    ))
    .unwrap();
    let aggregate = &aggregate_eligibility(&ir).aggregates[0];

    assert_eq!(aggregate.operation, AggregateOp::Count);
    assert_eq!(aggregate.channel, None);
    assert_eq!(aggregate.selected_cells, 4);
    assert_eq!(
        aggregate.candidate_shape,
        AggregateCandidateShape::PrecomputedRegionSelection
    );
}

#[test]
fn every_current_aggregate_op_is_eligible() {
    let ir = lower(&aggregate_model(
        Region::new("all")
            .on_field("Terrain")
            .mask(vec![true, true, true, true]),
        vec![
            Aggregate::sum("sum", "all", "height"),
            Aggregate::mean("mean", "all", "height"),
            Aggregate::min("min", "all", "height"),
            Aggregate::max("max", "all", "height"),
            Aggregate::count("count", "all"),
        ],
    ))
    .unwrap();
    let report = aggregate_eligibility(&ir);

    assert_eq!(report.aggregates.len(), 5);
    assert_eq!(report.eligible_count(), 5);
    assert!(report.aggregates.iter().all(|a| a.eligible));
}

#[test]
fn display_renders_stable_provenance() {
    let ir = lower(&aggregate_model(
        Region::new("north")
            .on_field("Terrain")
            .mask(vec![true, true, false, false]),
        vec![Aggregate::sum("north_height", "north", "height")],
    ))
    .unwrap();
    let rendered = aggregate_eligibility(&ir).to_string();

    assert!(rendered.contains("aggregate optimization eligibility"));
    assert!(rendered.contains("AGGREGATE `north_height`"));
    assert!(rendered.contains("ELIGIBLE"));
    assert!(rendered.contains("candidate: precomputed region selection"));
    assert!(rendered.contains("mask: boolean"));
}

#[test]
fn non_aggregate_models_have_empty_report_and_unaffected_plan() {
    use conflux_core::{col, lit, Rule, Table};

    let mut store = Table::new("T", 1);
    store.stock("x", vec![0.0]);
    let mut model = Model::new("world");
    model.add_table(store);
    model.add_rule(Rule::new("tick").on("T").propose("x", col("x") + lit(1.0)));
    let ir = lower(&model).unwrap();

    let report = aggregate_eligibility(&ir);
    assert!(report.aggregates.is_empty());
    assert_eq!(report.eligible_count(), 0);
    assert_eq!(plan(&ir).rules.len(), 1);
}
